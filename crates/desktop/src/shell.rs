use std::collections::HashSet;

use probe_core::{FolderKey, RequestKey};

const MIN_SIDEBAR_WIDTH: f32 = 180.0;
const MAX_SIDEBAR_WIDTH: f32 = 520.0;
const MIN_RESPONSE_HEIGHT: f32 = 120.0;
/// Largest share of the window the response pane may occupy.
const MAX_RESPONSE_HEIGHT_RATIO: f32 = 0.75;
/// Title bar, request tabs, and the request editor's minimum height.
const MIN_ABOVE_RESPONSE_HEIGHT: f32 = 190.0;
const MIN_RESPONSE_WIDTH: f32 = 240.0;
const MAX_RESPONSE_WIDTH: f32 = 760.0;
pub(crate) const DEFAULT_SIDEBAR_WIDTH: f32 = 260.0;
pub(crate) const DEFAULT_RESPONSE_HEIGHT: f32 = 220.0;
pub(crate) const DEFAULT_RESPONSE_WIDTH: f32 = 440.0;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum PaneLayout {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResizePane {
    Sidebar,
    Response,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum OverviewTab {
    Collection,
    Folder(FolderKey),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum OpenTab {
    Request(RequestKey),
    Overview(OverviewTab),
}

impl From<RequestKey> for OpenTab {
    fn from(key: RequestKey) -> Self {
        Self::Request(key)
    }
}

impl From<OverviewTab> for OpenTab {
    fn from(tab: OverviewTab) -> Self {
        Self::Overview(tab)
    }
}

#[derive(Debug)]
pub(crate) struct ShellState {
    open_tabs: Vec<OpenTab>,
    // Cached projections for request and overview-specific adapters.
    tabs: Vec<RequestKey>,
    overview_tabs: Vec<OverviewTab>,
    active: Option<OpenTab>,
    collapsed_folders: HashSet<FolderKey>,
    selected_environment: Option<String>,
    pub(crate) sidebar_width: f32,
    pub(crate) sidebar_collapsed: bool,
    pub(crate) response_height: f32,
    pub(crate) response_width: f32,
    pub(crate) pane_layout: PaneLayout,
    pub(crate) resizing: Option<ResizePane>,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            open_tabs: Vec::new(),
            tabs: Vec::new(),
            overview_tabs: Vec::new(),
            active: None,
            collapsed_folders: HashSet::new(),
            selected_environment: None,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            sidebar_collapsed: false,
            response_height: DEFAULT_RESPONSE_HEIGHT,
            response_width: DEFAULT_RESPONSE_WIDTH,
            pane_layout: PaneLayout::Vertical,
            resizing: None,
        }
    }
}

fn max_response_height(window_height: f32) -> f32 {
    let ratio_limit = window_height * MAX_RESPONSE_HEIGHT_RATIO;
    let above_limit = window_height - MIN_ABOVE_RESPONSE_HEIGHT;
    ratio_limit.min(above_limit).max(MIN_RESPONSE_HEIGHT)
}

impl ShellState {
    pub(crate) fn open_tabs(&self) -> &[OpenTab] {
        &self.open_tabs
    }

    pub(crate) fn overview_tabs(&self) -> &[OverviewTab] {
        &self.overview_tabs
    }

    pub(crate) fn active_overview(&self) -> Option<OverviewTab> {
        match self.active {
            Some(OpenTab::Overview(tab)) => Some(tab),
            _ => None,
        }
    }

    pub(crate) fn open_overview(&mut self, tab: OverviewTab) {
        if !self.overview_tabs.contains(&tab) {
            self.overview_tabs.push(tab);
            self.open_tabs.push(tab.into());
        }
        self.active = Some(tab.into());
    }

    pub(crate) fn close_overview(&mut self, tab: OverviewTab) {
        let Some(index) = self.overview_tabs.iter().position(|open| *open == tab) else {
            return;
        };
        let open_index = self
            .open_tabs
            .iter()
            .position(|open| *open == OpenTab::Overview(tab))
            .unwrap_or(index);
        self.overview_tabs.remove(index);
        self.open_tabs
            .retain(|open| *open != OpenTab::Overview(tab));
        if self.active == Some(tab.into()) {
            self.select_neighbor_after_close(open_index);
        }
    }

    pub(crate) fn tabs(&self) -> &[RequestKey] {
        &self.tabs
    }

    pub(crate) const fn active_tab(&self) -> Option<RequestKey> {
        match self.active {
            Some(OpenTab::Request(key)) => Some(key),
            _ => None,
        }
    }

    pub(crate) fn open_request(&mut self, key: RequestKey) {
        self.insert_tab(key);
        self.activate_tab(key);
    }

    /// Appends `key` when it is not already open, without changing the active tab.
    pub(crate) fn insert_tab(&mut self, key: RequestKey) {
        if !self.tabs.contains(&key) {
            self.tabs.push(key);
            self.open_tabs.push(key.into());
        }
    }

    /// Selects `key` when that tab is already open.
    pub(crate) fn activate_tab(&mut self, key: RequestKey) {
        if self.tabs.contains(&key) {
            self.active = Some(key.into());
        }
    }

    pub(crate) fn close_tab(&mut self, key: RequestKey) {
        let Some(index) = self.tabs.iter().position(|tab| *tab == key) else {
            return;
        };
        let open_index = self
            .open_tabs
            .iter()
            .position(|open| *open == OpenTab::Request(key))
            .unwrap_or(index);
        let active = self.active_open_tab();
        self.tabs.remove(index);
        self.open_tabs.retain(|open| *open != OpenTab::Request(key));
        if active == Some(OpenTab::Request(key)) {
            self.select_neighbor_after_close(open_index);
        }
    }

    fn select_neighbor_after_close(&mut self, index: usize) {
        self.active = self
            .open_tabs
            .get(index)
            .or_else(|| index.checked_sub(1).and_then(|i| self.open_tabs.get(i)))
            .copied();
    }

    pub(crate) fn active_open_tab(&self) -> Option<OpenTab> {
        self.active
    }

    /// Restores saved order, retaining tabs without persistent locators at the end.
    pub(crate) fn restore_tab_order(&mut self, order: impl IntoIterator<Item = OpenTab>) {
        let mut restored = Vec::with_capacity(self.open_tabs.len());
        for tab in order {
            if self.open_tabs.contains(&tab) && !restored.contains(&tab) {
                restored.push(tab);
            }
        }
        let remaining: Vec<_> = self
            .open_tabs
            .iter()
            .copied()
            .filter(|tab| !restored.contains(tab))
            .collect();
        restored.extend(remaining);
        self.open_tabs = restored;
        self.refresh_tab_lists();
    }

    fn refresh_tab_lists(&mut self) {
        self.tabs = self
            .open_tabs
            .iter()
            .filter_map(|tab| match tab {
                OpenTab::Request(key) => Some(*key),
                _ => None,
            })
            .collect();
        self.overview_tabs = self
            .open_tabs
            .iter()
            .filter_map(|tab| match tab {
                OpenTab::Overview(tab) => Some(*tab),
                _ => None,
            })
            .collect();
    }

    /// Moves an open tab to the side of another open tab without changing selection.
    pub(crate) fn move_tab(
        &mut self,
        source: impl Into<OpenTab>,
        target: impl Into<OpenTab>,
        before: bool,
    ) -> bool {
        let source = source.into();
        let target = target.into();
        let Some(from) = self.open_tabs.iter().position(|tab| *tab == source) else {
            return false;
        };
        let Some(target_index) = self.open_tabs.iter().position(|tab| *tab == target) else {
            return false;
        };
        let insertion = target_index + usize::from(!before);
        let destination = insertion.saturating_sub(usize::from(from < insertion));
        if from == destination {
            return false;
        }
        self.open_tabs.remove(from);
        self.open_tabs.insert(destination, source);
        self.refresh_tab_lists();
        true
    }

    pub(crate) fn toggle_folder(&mut self, key: FolderKey) {
        if !self.collapsed_folders.remove(&key) {
            self.collapsed_folders.insert(key);
        }
    }

    pub(crate) fn folder_is_expanded(&self, key: FolderKey) -> bool {
        !self.collapsed_folders.contains(&key)
    }

    pub(crate) fn collapsed_folders(&self) -> impl Iterator<Item = FolderKey> + '_ {
        self.collapsed_folders.iter().copied()
    }

    pub(crate) fn collapse_folder(&mut self, key: FolderKey) {
        self.collapsed_folders.insert(key);
    }

    pub(crate) fn expand_folder(&mut self, key: FolderKey) {
        self.collapsed_folders.remove(&key);
    }

    pub(crate) fn selected_environment(&self) -> Option<&str> {
        self.selected_environment.as_deref()
    }

    pub(crate) fn select_environment(&mut self, environment: Option<String>) {
        self.selected_environment = environment;
    }

    pub(crate) fn resize_sidebar(&mut self, position: f32) {
        self.sidebar_width = position.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
    }

    pub(crate) fn toggle_sidebar(&mut self) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
    }

    pub(crate) fn resize_response(&mut self, window_height: f32, position: f32) {
        self.response_height = (window_height - position)
            .clamp(MIN_RESPONSE_HEIGHT, max_response_height(window_height));
    }

    /// Height to draw, limited to [`MAX_RESPONSE_HEIGHT_RATIO`] of `window_height`.
    pub(crate) fn response_height_for_window(&self, window_height: f32) -> f32 {
        self.response_height
            .clamp(MIN_RESPONSE_HEIGHT, max_response_height(window_height))
    }

    pub(crate) fn resize_response_width(&mut self, window_width: f32, position: f32) {
        self.response_width =
            (window_width - position).clamp(MIN_RESPONSE_WIDTH, MAX_RESPONSE_WIDTH);
    }

    pub(crate) fn set_pane_layout(&mut self, layout: PaneLayout) {
        self.pane_layout = layout;
    }

    pub(crate) fn restore_pane_sizes(
        &mut self,
        sidebar_width: f32,
        response_height: f32,
        response_width: f32,
    ) {
        self.sidebar_width = sidebar_width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        self.response_height = response_height.max(MIN_RESPONSE_HEIGHT);
        self.response_width = response_width.clamp(MIN_RESPONSE_WIDTH, MAX_RESPONSE_WIDTH);
    }

    pub(crate) fn reset_for_workspace(&mut self) {
        self.open_tabs.clear();
        self.tabs.clear();
        self.overview_tabs.clear();
        self.active = None;
        self.collapsed_folders.clear();
        self.resizing = None;
    }
}

#[cfg(test)]
mod tests {
    use probe_core::{Collection, CollectionItem, Folder, Request, Workspace, WorkspaceItemRef};

    use super::{PaneLayout, ShellState};

    fn keys() -> (
        probe_core::RequestKey,
        probe_core::RequestKey,
        probe_core::FolderKey,
    ) {
        let workspace = Workspace::from_collection(Collection {
            items: vec![
                CollectionItem::Request(Request::default()),
                CollectionItem::Request(Request::default()),
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
    fn overview_tabs_reuse_close_and_reset_without_selecting_requests() {
        let (request, _, folder) = keys();
        let mut state = ShellState::default();
        state.open_request(request);
        let collection = super::OverviewTab::Collection;
        let folder = super::OverviewTab::Folder(folder);
        state.open_overview(collection);
        state.open_overview(folder);
        state.open_overview(collection);
        assert_eq!(state.overview_tabs(), &[collection, folder]);
        assert_eq!(state.active_overview(), Some(collection));
        assert_eq!(state.active_tab(), None);
        state.close_overview(collection);
        assert_eq!(state.active_overview(), Some(folder));
        state.close_overview(folder);
        assert_eq!(state.active_tab(), Some(request));
        state.open_overview(collection);
        state.open_request(request);
        assert_eq!(state.active_overview(), None);
        state.reset_for_workspace();
        assert!(state.overview_tabs().is_empty());
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
    fn moving_tabs_preserves_the_active_request_and_rejects_missing_tabs() {
        let (first, second, _) = keys();
        let mut state = ShellState::default();
        state.open_request(first);
        state.open_request(second);
        assert!(state.move_tab(second, first, true));
        assert_eq!(state.tabs(), &[second, first]);
        assert_eq!(state.active_tab(), Some(second));
        assert!(!state.move_tab(second, first, true));
        assert!(state.move_tab(second, first, false));
        assert_eq!(state.tabs(), &[first, second]);
        state.close_tab(first);
        assert!(!state.move_tab(first, second, true));
        assert!(!state.move_tab(second, first, true));
    }

    #[test]
    fn mixed_tab_neighbors_follow_visual_order_without_changing_selection_on_drag() {
        let (first, second, folder) = keys();
        let collection = super::OverviewTab::Collection;
        let folder = super::OverviewTab::Folder(folder);
        let mut state = ShellState::default();
        state.open_request(first);
        state.open_overview(collection);
        state.open_request(second);
        state.open_overview(folder);
        assert!(state.move_tab(folder, first, true));
        assert_eq!(state.active_overview(), Some(folder));
        state.close_overview(folder);
        assert_eq!(state.active_tab(), Some(first));
        state.close_tab(first);
        assert_eq!(state.active_overview(), Some(collection));
        state.close_tab(second);
        assert_eq!(state.active_overview(), Some(collection));
        state.close_overview(collection);
        assert_eq!(state.active_open_tab(), None);
        assert!(state.open_tabs().is_empty());
    }

    #[test]
    fn folders_and_pane_sizes_are_constrained() {
        let (_, _, folder) = keys();
        let mut state = ShellState::default();
        assert!(state.folder_is_expanded(folder));
        state.toggle_folder(folder);
        assert!(!state.folder_is_expanded(folder));
        state.expand_folder(folder);
        assert!(state.folder_is_expanded(folder));
        state.collapse_folder(folder);
        assert!(!state.folder_is_expanded(folder));

        state.resize_sidebar(20.0);
        state.resize_response(800.0, 790.0);
        assert_eq!(state.sidebar_width, 180.0);
        assert_eq!(state.response_height, 120.0);
        state.resize_response(800.0, 0.0);
        assert_eq!(state.response_height, 600.0);
        state.resize_response(560.0, 0.0);
        assert_eq!(state.response_height, 370.0);
        state.response_height = 900.0;
        assert_eq!(state.response_height_for_window(800.0), 600.0);

        state.resize_response_width(1000.0, 990.0);
        state.set_pane_layout(PaneLayout::Horizontal);
        assert_eq!(state.response_width, 240.0);
        assert_eq!(state.pane_layout, PaneLayout::Horizontal);

        state.toggle_sidebar();
        assert!(state.sidebar_collapsed);
        state.toggle_sidebar();
        assert!(!state.sidebar_collapsed);

        state.select_environment(Some("development".to_owned()));
        assert_eq!(state.selected_environment(), Some("development"));
        state.reset_for_workspace();
        assert_eq!(state.selected_environment(), Some("development"));
    }
}
