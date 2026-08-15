use std::collections::HashSet;

use probe_core::{FolderKey, RequestKey};

const MIN_SIDEBAR_WIDTH: f32 = 180.0;
const MAX_SIDEBAR_WIDTH: f32 = 520.0;
const MIN_RESPONSE_HEIGHT: f32 = 120.0;
const MAX_RESPONSE_HEIGHT: f32 = 560.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResizePane {
    Sidebar,
    Response,
}

#[derive(Debug)]
pub(crate) struct ShellState {
    tabs: Vec<RequestKey>,
    active_tab: Option<RequestKey>,
    collapsed_folders: HashSet<FolderKey>,
    pub(crate) sidebar_width: f32,
    pub(crate) response_height: f32,
    pub(crate) resizing: Option<ResizePane>,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: None,
            collapsed_folders: HashSet::new(),
            sidebar_width: 260.0,
            response_height: 220.0,
            resizing: None,
        }
    }
}

impl ShellState {
    pub(crate) fn tabs(&self) -> &[RequestKey] {
        &self.tabs
    }

    pub(crate) const fn active_tab(&self) -> Option<RequestKey> {
        self.active_tab
    }

    pub(crate) fn open_request(&mut self, key: RequestKey) {
        if !self.tabs.contains(&key) {
            self.tabs.push(key);
        }
        self.active_tab = Some(key);
    }

    pub(crate) fn close_tab(&mut self, key: RequestKey) {
        let Some(index) = self.tabs.iter().position(|tab| *tab == key) else {
            return;
        };
        self.tabs.remove(index);
        if self.active_tab == Some(key) {
            self.active_tab = self
                .tabs
                .get(index)
                .or_else(|| index.checked_sub(1).and_then(|index| self.tabs.get(index)))
                .copied();
        }
    }

    pub(crate) fn toggle_folder(&mut self, key: FolderKey) {
        if !self.collapsed_folders.remove(&key) {
            self.collapsed_folders.insert(key);
        }
    }

    pub(crate) fn folder_is_expanded(&self, key: FolderKey) -> bool {
        !self.collapsed_folders.contains(&key)
    }

    pub(crate) fn resize_sidebar(&mut self, position: f32) {
        self.sidebar_width = position.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
    }

    pub(crate) fn resize_response(&mut self, window_height: f32, position: f32) {
        self.response_height =
            (window_height - position).clamp(MIN_RESPONSE_HEIGHT, MAX_RESPONSE_HEIGHT);
    }

    pub(crate) fn reset_for_workspace(&mut self) {
        self.tabs.clear();
        self.active_tab = None;
        self.collapsed_folders.clear();
        self.resizing = None;
    }
}

#[cfg(test)]
mod tests {
    use probe_core::{
        Collection, CollectionItem, Folder, HttpRequest, Workspace, WorkspaceItemRef,
    };

    use super::ShellState;

    fn keys() -> (
        probe_core::RequestKey,
        probe_core::RequestKey,
        probe_core::FolderKey,
    ) {
        let workspace = Workspace::from_collection(Collection {
            items: vec![
                CollectionItem::HttpRequest(HttpRequest::default()),
                CollectionItem::HttpRequest(HttpRequest::default()),
                CollectionItem::Folder(Folder::default()),
            ],
            ..Collection::default()
        });
        let [
            WorkspaceItemRef::Request(first),
            WorkspaceItemRef::Request(second),
            WorkspaceItemRef::Folder(folder),
        ] = workspace.root_items()
        else {
            panic!("fixture must retain its item kinds");
        };
        (*first, *second, *folder)
    }

    #[test]
    fn opening_requests_deduplicates_tabs_and_selects_them() {
        let (first, second, _) = keys();
        let mut state = ShellState::default();
        state.open_request(first);
        state.open_request(second);
        state.open_request(first);

        assert_eq!(state.tabs(), &[first, second]);
        assert_eq!(state.active_tab(), Some(first));
    }

    #[test]
    fn closing_active_tab_selects_a_neighbor() {
        let (first, second, _) = keys();
        let mut state = ShellState::default();
        state.open_request(first);
        state.open_request(second);
        state.close_tab(second);

        assert_eq!(state.tabs(), &[first]);
        assert_eq!(state.active_tab(), Some(first));
    }

    #[test]
    fn folders_and_pane_sizes_are_constrained() {
        let (_, _, folder) = keys();
        let mut state = ShellState::default();
        assert!(state.folder_is_expanded(folder));
        state.toggle_folder(folder);
        assert!(!state.folder_is_expanded(folder));

        state.resize_sidebar(20.0);
        state.resize_response(800.0, 790.0);
        assert_eq!(state.sidebar_width, 180.0);
        assert_eq!(state.response_height, 120.0);
    }
}
