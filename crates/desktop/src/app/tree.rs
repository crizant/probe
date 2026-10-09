use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_base::Button;
use probe_core::{ItemKind, Workspace, WorkspaceItemRef};

use crate::{
    components, shell::ShellState, theme::Theme, tree_search::TreeSearchMatches,
    user_config::ThemeMode,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TreeRow {
    pub(crate) item: WorkspaceItemRef,
    pub(crate) depth: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct TreeDrag {
    pub(crate) item: WorkspaceItemRef,
    pub(crate) label: String,
    pub(crate) icon: Option<components::RequestIcon>,
    pub(crate) theme_mode: ThemeMode,
}

pub(crate) struct TreeRowSpec {
    pub(crate) item: WorkspaceItemRef,
    pub(crate) selector: String,
    pub(crate) label: String,
    pub(crate) icon: Option<components::RequestIcon>,
    pub(crate) depth: usize,
    pub(crate) selected: bool,
}

impl Render for TreeDrag {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_preference(self.theme_mode, window.appearance());
        let mut preview = div()
            .px(px(theme.metrics.spacing_2))
            .py(px(theme.metrics.spacing_1))
            .flex()
            .items_center()
            .gap(px(theme.metrics.spacing_1))
            .rounded(px(theme.metrics.radius_small))
            .bg(theme.colors.surfaces.overlay)
            .border_1()
            .border_color(theme.colors.borders.standard)
            .text_size(px(theme.typography.caption_size));
        if let Some(icon) = &self.icon {
            preview = preview.child(components::request_icon(theme, icon));
        } else if self.item.kind() == ItemKind::Folder {
            preview = preview.child(components::tree_folder_icon(theme, false, false));
        }
        preview.child(self.label.clone())
    }
}

pub(crate) fn tree_row_button(
    theme: Theme,
    id: impl Into<gpui::ElementId>,
    depth: usize,
    selected: bool,
) -> Button {
    Button::new(id)
        .focusable(true)
        .tab_stop(true)
        .key_context("RequestTree")
        .w_full()
        .h(px(theme.metrics.tree_row_height))
        .pl(px(tree_level_indent(theme, depth)))
        .pr(px(theme.metrics.spacing_1))
        .flex()
        .items_center()
        .gap(px(theme.metrics.spacing_1))
        .overflow_hidden()
        .rounded(px(theme.metrics.radius_small))
        .when(selected, |row| {
            row.bg(theme.colors.selection.inactive_background)
                .text_color(theme.colors.text.primary)
        })
        .when(!selected, |row| {
            row.hover(move |row| row.bg(theme.colors.surfaces.window))
        })
        .cursor_pointer()
}

pub(crate) fn tree_disclosure_width(theme: Theme) -> f32 {
    theme.metrics.icon_standard * 1.5
}

pub(crate) fn tree_level_indent(theme: Theme, depth: usize) -> f32 {
    theme.metrics.spacing_1 + depth as f32 * tree_disclosure_width(theme)
}

pub(crate) fn tree_hierarchy_guides(theme: Theme, depth: usize, selected: bool) -> gpui::Div {
    let mut guides = div().absolute().top(px(0.0)).bottom(px(0.0)).left(px(0.0));
    let color = if selected {
        theme.colors.selection.active_foreground.opacity(0.22)
    } else {
        theme.colors.borders.standard
    };
    for level in 0..depth {
        guides = guides.child(
            div()
                .absolute()
                .top(px(0.0))
                .bottom(px(0.0))
                .left(px(tree_level_indent(theme, level)
                    + tree_disclosure_width(theme) / 2.0
                    - theme.metrics.spacing_1 / 2.0
                    - 0.5))
                .w(px(1.0))
                .bg(color),
        );
    }
    guides
}

pub(crate) fn flatten_visible_tree_rows(
    workspace: &Workspace,
    items: &[WorkspaceItemRef],
    depth: usize,
    shell: &ShellState,
    filter: Option<&TreeSearchMatches>,
    rows: &mut Vec<TreeRow>,
) {
    for item in items {
        if filter.is_some_and(|hits| !hits.contains(*item)) {
            continue;
        }
        rows.push(TreeRow { item: *item, depth });
        if let WorkspaceItemRef::Folder(key) = item
            && shell.folder_is_expanded(*key)
            && let Some(folder) = workspace.folder(*key)
        {
            flatten_visible_tree_rows(workspace, &folder.children, depth + 1, shell, filter, rows);
        }
    }
}
