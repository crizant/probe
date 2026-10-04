use super::*;
use crate::app::documentation::documentation_text;

pub(super) fn documentation_sections(
    theme: Theme,
    first_label: &'static str,
    values: [Option<&str>; 2],
    ids: [gpui::ElementId; 2],
    on_change: impl Fn(bool, gpui::SharedString, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Div {
    let on_change = Rc::new(on_change);
    let mut sections = div()
        .size_full()
        .min_h(px(0.0))
        .flex()
        .flex_col()
        .gap(px(theme.metrics.spacing_2));
    for (index, ((label, text), id)) in [first_label, "Documentation"]
        .into_iter()
        .zip(values)
        .zip(ids)
        .enumerate()
    {
        let on_change = on_change.clone();
        sections = sections.child(
            div()
                .min_h(px(0.0))
                .when(index == 0, |section| section.flex_none())
                .when(index == 1, |section| section.flex_1())
                .flex()
                .flex_col()
                .gap(px(theme.metrics.spacing_1))
                .child(
                    div()
                        .text_size(px(theme.typography.caption_size))
                        .text_color(theme.colors.text.primary)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(label),
                )
                .child(
                    div()
                        .id(if index == 0 {
                            "documentation-first-editor"
                        } else {
                            "documentation-docs-editor"
                        })
                        .debug_selector(move || {
                            if index == 0 {
                                "documentation-first-editor".into()
                            } else {
                                "documentation-docs-editor".into()
                            }
                        })
                        .when(index == 0, |editor| {
                            editor.h(px(theme.metrics.control_height * 2.0)).flex_none()
                        })
                        .when(index == 1, |editor| editor.flex_1().min_h(px(120.0)))
                        .w_full()
                        .child(components::documentation_text_input(
                            theme,
                            id,
                            text.unwrap_or_default().to_owned(),
                            label,
                            move |value, window, cx| on_change(index == 1, value, window, cx),
                        )),
                ),
        );
    }
    sections
}

impl ProbeApp {
    pub(super) fn render_overview(
        &self,
        tab: crate::shell::OverviewTab,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let mut overview = div()
            .id("documentation-overview")
            .debug_selector(|| "documentation-overview".into())
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .p(px(theme.metrics.spacing_2))
            .bg(theme.colors.surfaces.editor);
        let Some(loaded) = &self.loaded_workspace else {
            return div().flex_1().child(overview);
        };
        let Some(target) = self.overview_target(tab) else {
            return div().flex_1();
        };
        let content = self
            .overview_drafts
            .get(&target)
            .filter(|draft| draft.is_dirty())
            .map(|draft| draft.current.clone())
            .or_else(|| self.overview_content(&target))
            .unwrap_or_default();
        let (name, label) = match tab {
            crate::shell::OverviewTab::Collection => {
                let metadata = loaded.workspace().metadata();
                (metadata.name.as_deref().unwrap_or("Collection"), "Summary")
            }
            crate::shell::OverviewTab::Folder(key) => {
                let Some(folder) = loaded.workspace().folder(key) else {
                    return div().flex_1().child(overview);
                };
                (
                    folder.metadata.name.as_deref().unwrap_or("Folder"),
                    "Description",
                )
            }
        };
        let icon = match tab {
            crate::shell::OverviewTab::Collection => components::collection_icon(theme),
            crate::shell::OverviewTab::Folder(_) => {
                components::tree_folder_icon(theme, false, false)
            }
        }
        .text_color(theme.colors.text.secondary);
        let title = match tab {
            crate::shell::OverviewTab::Collection => components::truncated_label(name.to_owned())
                .flex_1()
                .min_w(px(0.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(theme.typography.caption_size))
                .into_any_element(),
            crate::shell::OverviewTab::Folder(key) => {
                let folders = loaded
                    .folder_selector(key)
                    .map(|selector| folder_ancestor_selectors(loaded, selector))
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|selector| loaded.folder_key(selector))
                    .collect::<Vec<_>>();
                self.render_editor_breadcrumb(&folders, None, "folder-breadcrumb", theme, cx)
                    .into_any_element()
            }
        };
        let dirty = self
            .overview_drafts
            .get(&target)
            .is_some_and(|draft| draft.is_dirty());
        let busy = self.documentation_save_task.is_some();

        let edit_view = cx.weak_entity();
        let edit_target = target.clone();
        let ids = match tab {
            crate::shell::OverviewTab::Collection => {
                ["collection-summary".into(), "collection-docs".into()]
            }
            crate::shell::OverviewTab::Folder(key) => [
                ("folder-description", key.slot()).into(),
                ("folder-docs", key.slot()).into(),
            ],
        };
        let fields = documentation_sections(
            theme,
            label,
            [
                documentation_text(content.first.as_ref()),
                documentation_text(content.docs.as_ref()),
            ],
            ids,
            move |docs, value, _, cx| {
                let _ = edit_view.update(cx, |view, cx| {
                    view.edit_overview(edit_target.clone(), docs, value.to_string(), cx)
                });
            },
        );
        overview = overview.child(
            div()
                .flex_none()
                .h(px(theme.metrics.control_height))
                .w_full()
                .mb(px(theme.metrics.spacing_2))
                .flex()
                .items_center()
                .child(
                    components::tree_icon_slot(theme, icon)
                        .ml(px(theme.metrics.spacing_1))
                        .mr(px(theme.metrics.spacing_2)),
                )
                .child(title)
                .child(self.render_save_button(theme, "Save documentation", dirty, busy, cx)),
        );
        div()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .flex()
            .child(overview.child(fields))
    }
}
