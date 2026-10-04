use std::collections::{BTreeMap, BTreeSet, HashMap};

use probe_core::{Request, RequestKey};
use probe_opencollection::LoadedWorkspace;

#[derive(Clone, Copy, Debug)]
pub(crate) struct LocalRequestState<'a> {
    pub(crate) selector: &'a str,
    pub(crate) baseline: &'a Request,
    pub(crate) local: &'a Request,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SynchronizationConflict {
    Modified {
        selector: String,
        fields: Vec<&'static str>,
    },
    Deleted {
        selector: String,
    },
    AmbiguousRename {
        selector: String,
    },
}

impl SynchronizationConflict {
    pub(crate) fn description(&self) -> String {
        match self {
            Self::Modified { selector, fields } => {
                format!(
                    "{selector} changed locally and on disk ({})",
                    fields.join(", ")
                )
            }
            Self::Deleted { selector } => {
                format!("{selector} was deleted on disk while it has local changes")
            }
            Self::AmbiguousRename { selector } => {
                format!("the rename of {selector} could not be identified safely")
            }
        }
    }
}

pub(crate) struct ReconciledWorkspace {
    pub(crate) workspace: LoadedWorkspace,
    /// The on-disk version of every request in `workspace`, keyed by its fresh key.
    pub(crate) baselines: Vec<(RequestKey, Request)>,
    pub(crate) selector_remaps: BTreeMap<String, String>,
}

pub(crate) enum ReconcileResult {
    Applied(Box<ReconciledWorkspace>),
    Conflicted(Vec<SynchronizationConflict>),
}

/// Reconciles a freshly repository-loaded workspace with local editor drafts.
///
/// Filesystem data remains the persistence baseline. Local changes are merged one
/// request field at a time when the disk changed a different field. Any overlap is
/// returned to the desktop for an explicit user decision.
pub(crate) fn reconcile(
    local: &[LocalRequestState<'_>],
    mut fresh: LoadedWorkspace,
    rename_hints: &BTreeMap<String, String>,
) -> ReconcileResult {
    let mut claimed = BTreeSet::new();
    let mut selector_remaps = rename_hints.clone();
    let mut conflicts = Vec::new();
    // Merges are applied after matching so every lookup sees the disk version.
    let mut merges = BTreeMap::new();
    let index = MatchIndex::new(local, &fresh);

    for state in local {
        let target = find_target_selector(
            state,
            &index,
            &fresh,
            rename_hints,
            &claimed,
            &mut conflicts,
        );
        let Some(target) = target else {
            if state.local != state.baseline {
                conflicts.push(SynchronizationConflict::Deleted {
                    selector: state.selector.to_owned(),
                });
            }
            continue;
        };
        claimed.insert(target.clone());
        selector_remaps.insert(state.selector.to_owned(), target.clone());

        let key = fresh
            .request_key(&target)
            .expect("fresh selector must resolve to a request key");
        if state.local == state.baseline {
            // A clean merge is exactly the disk request.
            merges.remove(&key);
            continue;
        }
        let disk = fresh
            .workspace()
            .request(key)
            .expect("fresh request key must remain valid");
        let (merged, fields) = Request::reconcile(state.baseline, state.local, disk);
        if fields.is_empty() {
            merges.insert(key, merged);
        } else {
            conflicts.push(SynchronizationConflict::Modified {
                selector: target,
                fields,
            });
        }
    }

    if !conflicts.is_empty() {
        return ReconcileResult::Conflicted(conflicts);
    }
    let mut merged_baselines = BTreeMap::new();
    for (key, merged) in merges {
        let slot = fresh
            .request_mut(key)
            .expect("fresh request key must remain valid");
        merged_baselines.insert(key, std::mem::replace(slot, merged));
    }
    let baselines = fresh
        .requests()
        .iter()
        .filter_map(|located| {
            let baseline = match merged_baselines.remove(&located.key()) {
                Some(baseline) => baseline,
                None => fresh.workspace().request(located.key())?.clone(),
            };
            Some((located.key(), baseline))
        })
        .collect();
    ReconcileResult::Applied(Box::new(ReconciledWorkspace {
        workspace: fresh,
        baselines,
        selector_remaps,
    }))
}

fn disk_request<'a>(fresh: &'a LoadedWorkspace, selector: &str) -> Option<&'a Request> {
    fresh
        .request_key(selector)
        .and_then(|key| fresh.workspace().request(key))
}

/// Fields that `Request` equality compares exactly, so equal requests always share
/// a key. Keys only narrow the search; every match is still confirmed with `==`.
type MatchKey<'a> = (Option<&'a str>, Option<&'a str>, Option<&'a str>);

fn match_key(request: &Request) -> MatchKey<'_> {
    (
        request.metadata.name.as_deref(),
        request.method.as_deref(),
        request.url.as_deref(),
    )
}

/// Former baselines and fresh disk requests grouped by [`MatchKey`], preserving
/// local and repository order within each group.
struct MatchIndex<'a> {
    baselines: HashMap<MatchKey<'a>, Vec<&'a LocalRequestState<'a>>>,
    disk: HashMap<MatchKey<'a>, Vec<(&'a str, &'a Request)>>,
}

impl<'a> MatchIndex<'a> {
    fn new(local: &'a [LocalRequestState<'a>], fresh: &'a LoadedWorkspace) -> Self {
        let mut baselines = HashMap::<_, Vec<_>>::with_capacity(local.len());
        for state in local {
            baselines
                .entry(match_key(state.baseline))
                .or_default()
                .push(state);
        }
        let mut disk = HashMap::<_, Vec<_>>::with_capacity(fresh.requests().len());
        for located in fresh.requests() {
            if let Some(request) = fresh.workspace().request(located.key()) {
                disk.entry(match_key(request))
                    .or_default()
                    .push((located.selector(), request));
            }
        }
        Self { baselines, disk }
    }

    fn states_with_baseline<'r>(
        &'r self,
        request: &'r Request,
    ) -> impl Iterator<Item = &'a LocalRequestState<'a>> + 'r {
        self.baselines
            .get(&match_key(request))
            .into_iter()
            .flatten()
            .copied()
            .filter(move |state| state.baseline == request)
    }

    fn disk_selectors_equal_to<'r>(
        &'r self,
        request: &'r Request,
    ) -> impl Iterator<Item = &'a str> + 'r {
        self.disk
            .get(&match_key(request))
            .into_iter()
            .flatten()
            .filter(move |(_, disk)| *disk == request)
            .map(|(selector, _)| *selector)
    }
}

fn find_target_selector(
    state: &LocalRequestState<'_>,
    index: &MatchIndex<'_>,
    fresh: &LoadedWorkspace,
    rename_hints: &BTreeMap<String, String>,
    claimed: &BTreeSet<String>,
    conflicts: &mut Vec<SynchronizationConflict>,
) -> Option<String> {
    if let Some(target) = hinted_selector(state.selector, rename_hints)
        && fresh.request_key(&target).is_some()
        && !claimed.contains(&target)
    {
        return Some(target);
    }
    if let Some(exact) = disk_request(fresh, state.selector) {
        let belongs_to_another_request = index
            .states_with_baseline(exact)
            .any(|other| other.selector != state.selector);
        if !belongs_to_another_request {
            return Some(state.selector.to_owned());
        }
    }

    let mut candidates = index
        .disk_selectors_equal_to(state.baseline)
        .filter(|selector| !claimed.contains(*selector));
    match (candidates.next(), candidates.next()) {
        (Some(selector), None) => Some(selector.to_owned()),
        (None, _) => None,
        _ if state.local != state.baseline => {
            conflicts.push(SynchronizationConflict::AmbiguousRename {
                selector: state.selector.to_owned(),
            });
            None
        }
        _ => None,
    }
}

/// Applies the longest hint whose source is the selector or one of its `/`-separated
/// ancestors.
fn hinted_selector(selector: &str, rename_hints: &BTreeMap<String, String>) -> Option<String> {
    std::iter::once(selector.len())
        .chain(selector.rmatch_indices('/').map(|(end, _)| end))
        .find_map(|end| {
            rename_hints
                .get(&selector[..end])
                .map(|to| format!("{to}{}", &selector[end..]))
        })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        LocalRequestState, ReconcileResult, SynchronizationConflict, hinted_selector, reconcile,
    };

    fn fixture_copy() -> PathBuf {
        named_fixture_copy("phase1-bundled.yml")
    }

    fn named_fixture_copy(name: &str) -> PathBuf {
        static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/opencollection")
            .join(name);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "probe-sync-{}-{timestamp}-{}.yml",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::copy(source, &path).unwrap();
        path
    }

    struct OwnedState {
        selector: String,
        baseline: probe_core::Request,
        local: probe_core::Request,
    }

    impl OwnedState {
        fn state(&self) -> LocalRequestState<'_> {
            LocalRequestState {
                selector: &self.selector,
                baseline: &self.baseline,
                local: &self.local,
            }
        }
    }

    fn request_state(
        workspace: &probe_opencollection::LoadedWorkspace,
        index: usize,
    ) -> OwnedState {
        let located = &workspace.requests()[index];
        let request = workspace
            .workspace()
            .request(located.key())
            .unwrap()
            .clone();
        OwnedState {
            selector: located.selector().to_owned(),
            baseline: request.clone(),
            local: request,
        }
    }

    #[test]
    fn merges_non_overlapping_local_and_disk_changes() {
        let path = named_fixture_copy("documentation.yml");
        let original = probe_opencollection::load_workspace(&path).unwrap();
        let mut state = request_state(&original, 0);
        state.local.url = Some("https://local.example".to_owned());
        state.local.metadata.description = Some(probe_core::Documentation::Text(
            "Local description".to_owned(),
        ));
        state.local.docs = Some("Local docs".to_owned());

        let mut source = fs::read_to_string(&path).unwrap();
        source = source.replacen("method: POST", "method: PATCH", 1);
        fs::write(&path, source).unwrap();
        let fresh = probe_opencollection::load_workspace(&path).unwrap();
        let ReconcileResult::Applied(result) = reconcile(&[state.state()], fresh, &BTreeMap::new())
        else {
            panic!("non-overlapping changes should merge")
        };
        let request = result
            .workspace
            .workspace()
            .request(result.workspace.requests()[0].key())
            .unwrap();
        assert_eq!(request.url.as_deref(), Some("https://local.example"));
        assert_eq!(request.method.as_deref(), Some("PATCH"));
        assert_eq!(
            request.metadata.description,
            state.local.metadata.description
        );
        assert_eq!(request.docs, state.local.docs);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn reports_overlapping_field_changes() {
        for field in ["method", "description", "docs"] {
            let path = named_fixture_copy("documentation.yml");
            let original = probe_opencollection::load_workspace(&path).unwrap();
            let mut state = request_state(&original, 0);
            let source = fs::read_to_string(&path).unwrap();
            let source = if field == "method" {
                state.local.method = Some("GET".to_owned());
                source.replacen("method: POST", "method: PATCH", 1)
            } else if field == "description" {
                state.local.metadata.description = Some(probe_core::Documentation::Text(
                    "Local description".to_owned(),
                ));
                source.replacen("content: Creates a pet", "content: Disk description", 1)
            } else {
                state.local.docs = Some("Local docs".to_owned());
                source.replacen("docs: request docs stay a string", "docs: Disk docs", 1)
            };
            fs::write(&path, source).unwrap();
            let fresh = probe_opencollection::load_workspace(&path).unwrap();
            let ReconcileResult::Conflicted(conflicts) =
                reconcile(&[state.state()], fresh, &BTreeMap::new())
            else {
                panic!("overlapping {field} changes should conflict")
            };
            assert!(matches!(
                conflicts.as_slice(),
                [SynchronizationConflict::Modified { selector, fields }]
                    if selector == &state.selector && fields == &[field]
            ));
            fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn merges_local_graphql_body_with_disk_changes() {
        let path = named_fixture_copy("graphql-http.yml");
        let original = probe_opencollection::load_workspace(&path).unwrap();
        let mut state = request_state(&original, 0);
        state
            .local
            .apply_graphql_update(&probe_core::GraphqlUpdate {
                query: probe_core::FieldPatch::Set("query Local { viewer { id } }".to_owned()),
                ..probe_core::GraphqlUpdate::default()
            })
            .unwrap();

        let mut source = fs::read_to_string(&path).unwrap();
        source = source.replacen("method: POST", "method: GET", 1);
        fs::write(&path, source).unwrap();
        let fresh = probe_opencollection::load_workspace(&path).unwrap();
        let ReconcileResult::Applied(result) = reconcile(&[state.state()], fresh, &BTreeMap::new())
        else {
            panic!("a local GraphQL body change should merge with a disk method change")
        };
        let request = result
            .workspace
            .workspace()
            .request(result.workspace.requests()[0].key())
            .unwrap();
        assert_eq!(request.method.as_deref(), Some("GET"));
        assert_eq!(
            request
                .selected_graphql()
                .unwrap()
                .unwrap()
                .query
                .as_deref(),
            Some("query Local { viewer { id } }")
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn merges_non_overlapping_path_parameter_changes() {
        let path = fixture_copy();
        let original = probe_opencollection::load_workspace(&path).unwrap();
        let mut state = request_state(&original, 0);
        state.local.path_parameters = vec![probe_core::QueryParameter {
            name: "ownerId".to_owned(),
            value: "99".to_owned(),
            disabled: false,
        }];

        let mut source = fs::read_to_string(&path).unwrap();
        source = source.replacen("value: \"25\"", "value: \"50\"", 1);
        fs::write(&path, source).unwrap();
        let fresh = probe_opencollection::load_workspace(&path).unwrap();
        let ReconcileResult::Applied(result) = reconcile(&[state.state()], fresh, &BTreeMap::new())
        else {
            panic!("non-overlapping path parameter changes should merge")
        };
        let request = result
            .workspace
            .workspace()
            .request(result.workspace.requests()[0].key())
            .unwrap();
        assert_eq!(request.path_parameters[0].value, "99");
        assert_eq!(request.query_parameters[0].value, "50");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn remaps_a_dirty_request_through_a_confident_rename_hint() {
        let path = fixture_copy();
        let original = probe_opencollection::load_workspace(&path).unwrap();
        let mut state = request_state(&original, 1);
        state.local.url = Some("https://local.example".to_owned());
        let old = "renamed-request.yml".to_owned();
        state.selector.clone_from(&old);

        // Bundled selectors are structural, so emulate a watcher-provided selector rename
        // against another otherwise-identical request.
        let target = original.requests()[1].selector().to_owned();
        let mut hints = BTreeMap::new();
        hints.insert(old.clone(), target.clone());
        let fresh = probe_opencollection::load_workspace(&path).unwrap();
        let ReconcileResult::Applied(result) = reconcile(&[state.state()], fresh, &hints) else {
            panic!("a watcher rename pair should be authoritative")
        };
        assert_eq!(result.selector_remaps.get(&old), Some(&target));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn preserves_a_dirty_request_when_it_was_deleted() {
        let path = fixture_copy();
        let original = probe_opencollection::load_workspace(&path).unwrap();
        let mut state = request_state(&original, 0);
        state.baseline.url = Some("https://deleted.example".to_owned());
        state.local = state.baseline.clone();
        state.local.method = Some("POST".to_owned());
        let fresh = probe_opencollection::load_workspace(&path).unwrap();
        state.selector = "missing.yml".to_owned();

        let ReconcileResult::Conflicted(conflicts) =
            reconcile(&[state.state()], fresh, &BTreeMap::new())
        else {
            panic!("a dirty deletion should conflict")
        };
        assert!(matches!(
            conflicts.as_slice(),
            [SynchronizationConflict::Deleted { .. }]
        ));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn folder_rename_hints_remap_descendant_selectors() {
        let hints = BTreeMap::from([("old-folder".to_owned(), "new-folder".to_owned())]);
        assert_eq!(
            hinted_selector("old-folder/nested/request.yml", &hints).as_deref(),
            Some("new-folder/nested/request.yml")
        );
        assert_eq!(hinted_selector("old-folderish/request.yml", &hints), None);
    }

    #[test]
    fn bundled_deletion_remaps_the_surviving_structural_selector() {
        let original = probe_opencollection::load_workspace_from_str(
            "opencollection: 1.0.0\ninfo: { name: Test }\nbundled: true\nitems:\n  - info: { name: First, type: http }\n    http: { method: GET, url: https://first.example }\n  - info: { name: Second, type: http }\n    http: { method: GET, url: https://second.example }\n",
        )
        .unwrap();
        let local = [request_state(&original, 0), request_state(&original, 1)];
        let fresh = probe_opencollection::load_workspace_from_str(
            "opencollection: 1.0.0\ninfo: { name: Test }\nbundled: true\nitems:\n  - info: { name: Second, type: http }\n    http: { method: GET, url: https://second.example }\n",
        )
        .unwrap();

        let ReconcileResult::Applied(result) = reconcile(
            &local.iter().map(OwnedState::state).collect::<Vec<_>>(),
            fresh,
            &BTreeMap::new(),
        ) else {
            panic!("deleting a clean request should apply")
        };
        assert_eq!(
            result.selector_remaps.get("items/1").map(String::as_str),
            Some("items/0")
        );
        assert!(!result.selector_remaps.contains_key("items/0"));
        assert_eq!(result.workspace.workspace().request_count(), 1);
    }

    fn bundled(items: &[&str]) -> probe_opencollection::LoadedWorkspace {
        let mut source =
            "opencollection: 1.0.0\ninfo: { name: Test }\nbundled: true\nitems:\n".to_owned();
        for item in items {
            source.push_str(item);
        }
        probe_opencollection::load_workspace_from_str(&source).unwrap()
    }

    const SAME: &str = "  - info: { name: Same, type: http }\n    http: { method: GET, url: https://same.example }\n";
    const SAME_WITH_HEADER: &str = "  - info: { name: Same, type: http }\n    http: { method: GET, url: https://same.example, headers: [{ name: X-Id, value: \"1\" }] }\n";
    const OTHER: &str = "  - info: { name: Other, type: http }\n    http: { method: GET, url: https://other.example }\n";

    fn request_at<'a>(
        workspace: &'a probe_opencollection::LoadedWorkspace,
        selector: &str,
    ) -> &'a probe_core::Request {
        workspace
            .workspace()
            .request(workspace.request_key(selector).unwrap())
            .unwrap()
    }

    #[test]
    fn deleting_one_of_two_equal_requests_remaps_the_later_dirty_request() {
        let original = bundled(&[SAME, SAME, OTHER]);
        let mut local = [0, 1, 2].map(|index| request_state(&original, index));
        local[2].local.url = Some("https://local.example".to_owned());
        let fresh = bundled(&[SAME, OTHER]);

        let ReconcileResult::Applied(result) = reconcile(
            &local.iter().map(OwnedState::state).collect::<Vec<_>>(),
            fresh,
            &BTreeMap::new(),
        ) else {
            panic!("deleting a clean duplicate should apply")
        };
        assert_eq!(
            result.selector_remaps.get("items/0").map(String::as_str),
            Some("items/0")
        );
        assert!(!result.selector_remaps.contains_key("items/1"));
        assert_eq!(
            result.selector_remaps.get("items/2").map(String::as_str),
            Some("items/1")
        );
        assert_eq!(
            request_at(&result.workspace, "items/1").url.as_deref(),
            Some("https://local.example")
        );
    }

    #[test]
    fn equal_disk_candidates_make_only_a_dirty_rename_ambiguous() {
        let original = bundled(&[OTHER, OTHER, SAME]);
        let mut state = request_state(&original, 2);

        let fresh = bundled(&[SAME, SAME]);
        let ReconcileResult::Applied(result) = reconcile(&[state.state()], fresh, &BTreeMap::new())
        else {
            panic!("a clean request with equal candidates should be dropped without conflict")
        };
        assert!(result.selector_remaps.is_empty());

        state.local.url = Some("https://local.example".to_owned());
        let fresh = bundled(&[SAME, SAME]);
        let ReconcileResult::Conflicted(conflicts) =
            reconcile(&[state.state()], fresh, &BTreeMap::new())
        else {
            panic!("a dirty request with equal candidates should conflict")
        };
        assert!(matches!(
            conflicts.as_slice(),
            [
                SynchronizationConflict::AmbiguousRename { selector },
                SynchronizationConflict::Deleted { .. },
            ] if selector == "items/2"
        ));
    }

    #[test]
    fn rename_matching_requires_full_equality_beyond_name_method_and_url() {
        let original = bundled(&[OTHER, OTHER, SAME_WITH_HEADER]);
        let mut state = request_state(&original, 2);
        state.local.url = Some("https://local.example".to_owned());
        let fresh = bundled(&[SAME, SAME_WITH_HEADER]);

        let ReconcileResult::Applied(result) = reconcile(&[state.state()], fresh, &BTreeMap::new())
        else {
            panic!("only one disk request equals the baseline")
        };
        assert_eq!(
            result.selector_remaps.get("items/2").map(String::as_str),
            Some("items/1")
        );
        assert_eq!(
            request_at(&result.workspace, "items/0").url.as_deref(),
            Some("https://same.example")
        );
    }
}
