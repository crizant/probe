use super::*;
use probe_core::{Documentation, FieldPatch};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum OverviewTarget {
    Collection,
    Folder(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct OverviewContent {
    pub(super) first: Option<Documentation>,
    pub(super) docs: Option<Documentation>,
}

#[derive(Clone, Debug)]
pub(super) struct OverviewDraft {
    pub(super) original: OverviewContent,
    pub(super) current: OverviewContent,
}

impl OverviewContent {
    fn summary(&self) -> Option<String> {
        documentation_text(self.first.as_ref()).map(str::to_owned)
    }
}

impl OverviewDraft {
    pub(super) fn is_dirty(&self) -> bool {
        self.current != self.original
    }
}

pub(super) fn edit_documentation(value: &mut Option<Documentation>, text: String) {
    match value {
        Some(Documentation::Content { content, .. }) => *content = text,
        _ => *value = Some(Documentation::Text(text)),
    }
}

pub(super) fn documentation_text(value: Option<&Documentation>) -> Option<&str> {
    match value {
        Some(Documentation::Text(text)) => Some(text),
        Some(Documentation::Content { content, .. }) => Some(content),
        Some(Documentation::Null) | None => None,
    }
}

pub(super) fn patch<T: Clone + PartialEq>(old: &Option<T>, new: &Option<T>) -> FieldPatch<T> {
    if old == new {
        FieldPatch::Unchanged
    } else {
        FieldPatch::from_optional(new.clone())
    }
}

impl ProbeApp {
    pub(super) fn overview_target(&self, tab: crate::shell::OverviewTab) -> Option<OverviewTarget> {
        match tab {
            crate::shell::OverviewTab::Collection => Some(OverviewTarget::Collection),
            crate::shell::OverviewTab::Folder(key) => self
                .loaded_workspace
                .as_ref()?
                .folder_selector(key)
                .map(|selector| OverviewTarget::Folder(selector.to_owned())),
        }
    }

    pub(super) fn overview_content(&self, target: &OverviewTarget) -> Option<OverviewContent> {
        let loaded = self.loaded_workspace.as_ref()?;
        match target {
            OverviewTarget::Collection => {
                let metadata = loaded.workspace().metadata();
                Some(OverviewContent {
                    first: metadata.summary.clone().map(Documentation::Text),
                    docs: metadata.docs.clone(),
                })
            }
            OverviewTarget::Folder(selector) => {
                let folder = loaded.workspace().folder(loaded.folder_key(selector)?)?;
                Some(OverviewContent {
                    first: folder.metadata.description.clone(),
                    docs: folder.docs.clone(),
                })
            }
        }
    }

    pub(super) fn edit_overview(
        &mut self,
        target: OverviewTarget,
        docs: bool,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let Some(original) = self.overview_content(&target) else {
            return;
        };
        if self
            .overview_drafts
            .get(&target)
            .is_some_and(|draft| !draft.is_dirty())
        {
            self.overview_drafts.remove(&target);
        }
        let draft = self
            .overview_drafts
            .entry(target)
            .or_insert_with(|| OverviewDraft {
                current: original.clone(),
                original,
            });
        edit_documentation(
            if docs {
                &mut draft.current.docs
            } else {
                &mut draft.current.first
            },
            text,
        );
        cx.notify();
    }

    pub(super) fn has_dirty_overviews(&self) -> bool {
        self.overview_drafts.values().any(OverviewDraft::is_dirty)
    }

    pub(super) fn documentation_blocks_close_or_open(&self) -> bool {
        self.has_dirty_overviews() || self.documentation_save_task.is_some()
    }

    pub(super) fn complete_overview_draft_save(
        &mut self,
        target: &OverviewTarget,
        submitted: &OverviewContent,
    ) {
        if let Some(saved) = self.overview_content(target)
            && let Some(current) = self.overview_drafts.get_mut(target)
        {
            if current.current.first == submitted.first {
                current.current.first = saved.first.clone();
            }
            if current.current.docs == submitted.docs {
                current.current.docs = saved.docs.clone();
            }
            current.original = saved;
        }
    }

    pub(super) fn pending_overview_targets(&self, pending: &PendingClose) -> Vec<OverviewTarget> {
        self.overview_drafts
            .iter()
            .filter(|(target, draft)| {
                draft.is_dirty()
                    && match pending {
                        PendingClose::Tab(_) => false,
                        PendingClose::Overview(selected) => selected == *target,
                        PendingClose::OtherTabs {
                            keep: crate::shell::OpenTab::Overview(tab),
                        } => self.overview_target(*tab).as_ref() != Some(*target),
                        _ => true,
                    }
            })
            .map(|(target, _)| target.clone())
            .collect()
    }

    pub(super) fn request_close_overview(
        &mut self,
        tab: crate::shell::OverviewTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_tab_context_menu(cx);
        let Some(target) = self.overview_target(tab) else {
            return;
        };
        let pending = PendingClose::Overview(target);
        if self.pending_overview_targets(&pending).is_empty()
            && self.documentation_save_task.is_none()
        {
            self.shell.close_overview(tab);
            self.select_and_reveal_active_request_in_sidebar();
            self.reveal_active_tab();
            self.persist_session(cx);
            cx.notify();
        } else {
            self.prompt_unsaved(Vec::new(), pending, window, cx);
        }
    }

    pub(super) fn enqueue_documentation_save(&mut self, target: OverviewTarget) {
        if !self.pending_documentation_saves.contains(&target) {
            self.pending_documentation_saves.push_back(target);
        }
    }

    pub(super) fn start_next_documentation_save(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.loading || self.has_active_workspace_write() {
            return;
        }
        let Some(target) = self.pending_documentation_saves.pop_front() else {
            return;
        };
        let Some(draft) = self
            .overview_drafts
            .get(&target)
            .filter(|draft| draft.is_dirty())
            .cloned()
        else {
            self.start_next_documentation_save(window, cx);
            self.finish_pending_close_if_idle(window, cx);
            return;
        };
        let Some(current) = self.overview_content(&target) else {
            self.fail_documentation_save(
                "The folder no longer exists. Discard this documentation draft to continue.",
                cx,
            );
            return;
        };
        // Only overlapping edits conflict; unrelated disk changes remain untouched.
        if (draft.original.first != draft.current.first && current.first != draft.original.first)
            || (draft.original.docs != draft.current.docs && current.docs != draft.original.docs)
        {
            self.fail_documentation_save("Documentation changed on disk. Discard the draft and review the updated documentation before editing again.", cx);
            return;
        }
        let loaded = self
            .loaded_workspace
            .as_ref()
            .expect("overview content requires a workspace");
        let prepared = match &target {
            OverviewTarget::Collection => {
                loaded.prepare_collection_save(probe_core::CollectionUpdate {
                    summary: patch(&draft.original.summary(), &draft.current.summary()),
                    docs: patch(&draft.original.docs, &draft.current.docs),
                })
            }
            OverviewTarget::Folder(selector) => loaded.prepare_folder_save(
                selector,
                probe_core::FolderUpdate {
                    description: patch(&draft.original.first, &draft.current.first),
                    docs: patch(&draft.original.docs, &draft.current.docs),
                },
            ),
        };
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.fail_documentation_save(format!("Could not save documentation: {error}"), cx);
                return;
            }
        };
        let path = self.workspace_path.clone();
        self.documentation_save_task = Some(cx.spawn_in(window, async move |view, window| {
            let result = window
                .background_spawn(async move { prepared.execute() })
                .await;
            let _ = view.update_in(window, |view, window, cx| {
                view.documentation_save_task = None;
                let result = result.and_then(|saved| {
                    view.loaded_workspace
                        .as_mut()
                        .ok_or(probe_opencollection::SaveError::CommittedButNotIntegrated)?
                        .complete_documentation_save(saved)
                });
                match result {
                    Ok(()) => {
                        view.complete_overview_draft_save(&target, &draft.current);
                        view.show_toast(ToastIntent::Success, "Documentation saved.", cx);
                        view.start_next_documentation_save(window, cx);
                        view.start_next_request_save(window, cx);
                        view.start_next_environment_save(window, cx);
                    }
                    Err(probe_opencollection::SaveError::CommittedButNotIntegrated) => {
                        view.pending_documentation_saves.clear();
                        view.pending_close = None;
                        view.recover_committed_save(
                            path,
                            Some((target, draft.current)),
                            window,
                            cx,
                        );
                    }
                    Err(error) => view.fail_documentation_save(
                        format!("Could not save documentation: {error}"),
                        cx,
                    ),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn fail_documentation_save(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.pending_close = None;
        self.pending_documentation_saves.clear();
        self.show_toast(ToastIntent::Error, message, cx);
    }
}
