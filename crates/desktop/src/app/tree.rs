use gpui_kit::base::Button;
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
    relative,
};
use probe_core::{ItemKind, Workspace, WorkspaceItemRef};

use crate::{
    components,
    shell::ShellState,
    theme::{Theme, ThemeAppearance},
    tree_search::TreeSearchMatches,
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

pub(crate) enum TreeRowContent {
    Request(components::RequestIcon),
    Folder { expanded: bool },
    CollectionRoot,
}

/// One row composition for the sidebar and destination pickers. Callers add
/// interaction handlers to the button and decorations to the resulting wrapper.
pub(crate) struct TreeRowElement {
    theme: Theme,
    depth: usize,
    selected: bool,
    button: Button,
}

impl TreeRowElement {
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }

    pub(crate) fn map_button(mut self, build: impl FnOnce(Button) -> Button) -> Self {
        self.button = build(self.button);
        self
    }

    pub(crate) fn into_row(
        self,
        id: impl Into<gpui_kit::ElementId>,
        debug_selector: impl Into<String>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let debug_selector = debug_selector.into();
        let guide_prefix = debug_selector.clone();
        div()
            .id(id)
            .relative()
            .w_full()
            .h(px(self.theme.metrics.tree_row_height))
            .debug_selector(move || debug_selector.clone())
            .child(self.button)
            .when(self.depth > 0, |row| {
                row.child(tree_hierarchy_guides(
                    self.theme,
                    self.depth,
                    self.selected,
                    &guide_prefix,
                ))
            })
    }
}

pub(crate) fn tree_row(
    theme: Theme,
    id: impl Into<gpui_kit::ElementId>,
    depth: usize,
    selected: bool,
    label: impl Into<String>,
    content: TreeRowContent,
    disclosure: Option<Button>,
) -> TreeRowElement {
    let label = label.into();
    let (marker, label) = match content {
        TreeRowContent::Request(icon) => {
            let marker = components::request_icon(theme, &icon)
                .debug_selector(|| "request-tree-icon".into())
                .when(matches!(icon, components::RequestIcon::Graphql), |icon| {
                    icon.debug_selector(|| "request-tree-protocol-label".into())
                });
            let label = components::truncated_label(label)
                .flex_1()
                .text_color(theme.colors.text.primary)
                .when(selected, |label| {
                    label.debug_selector(|| "request-tree-label".into())
                });
            (marker, label)
        }
        TreeRowContent::Folder { expanded } => {
            let mut marker = div().flex_none().flex().items_center();
            if let Some(disclosure) = disclosure {
                marker = marker.child(
                    disclosure
                        .accessibility_label(format!(
                            "{} {label}",
                            if expanded { "Collapse" } else { "Expand" },
                        ))
                        .w(px(tree_disclosure_width(theme)))
                        .h(px(theme.metrics.tree_row_height))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            components::tree_disclosure_icon(theme, expanded)
                                .relative()
                                .left(px(-theme.metrics.spacing_1 / 2.0)),
                        ),
                );
            } else {
                marker = marker.child(div().flex_none().w(px(tree_disclosure_width(theme))));
            }
            marker = marker.child(components::tree_icon_slot(
                theme,
                components::tree_folder_icon(theme, expanded, false),
            ));
            let label = components::truncated_label(label)
                .flex_1()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD);
            (marker, label)
        }
        TreeRowContent::CollectionRoot => (
            components::tree_icon_slot(theme, components::tree_folder_icon(theme, true, false)),
            components::truncated_label(label)
                .flex_1()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD),
        ),
    };
    TreeRowElement {
        theme,
        depth,
        selected,
        button: tree_row_button(theme, id, depth, selected)
            .child(marker)
            .child(label),
    }
}

fn tree_row_button(
    theme: Theme,
    id: impl Into<gpui_kit::ElementId>,
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
        .line_height(relative(1.25))
        .rounded(px(theme.metrics.radius_small))
        .when(selected, |row| {
            let background = match theme.appearance {
                ThemeAppearance::Light => theme.colors.selection.inactive_background,
                ThemeAppearance::Dark => theme
                    .colors
                    .surfaces
                    .sidebar
                    .blend(theme.colors.borders.standard.alpha(0.7)),
            };
            row.bg(background).text_color(theme.colors.text.primary)
        })
        .when(!selected, |row| {
            row.hover(move |row| row.bg(theme.sidebar_hover()))
        })
        .cursor_pointer()
}

/// A destination picker shared by structure dialogs. State and callbacks belong
/// to the caller, independently of the sidebar's expansion and selection.
pub(crate) fn destination_tree(
    theme: Theme,
    rows: &[crate::structure_editor::SaveDestinationRow],
    selected_parent: Option<&str>,
    disabled: bool,
    on_select: impl Fn(String, &mut gpui_kit::App) + Clone + 'static,
    on_toggle: impl Fn(String, &mut gpui_kit::App) + Clone + 'static,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let mut tree = div()
        .id("save-destination-tree")
        .w_full()
        .max_h(px(theme.metrics.tree_row_height * 8.0))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .py(px(theme.metrics.spacing_1))
        .rounded(px(theme.metrics.radius_small))
        .border_1()
        .border_color(theme.colors.borders.standard)
        .bg(theme.colors.surfaces.sidebar)
        .font_family(theme.typography.interface_family)
        .text_size(px(theme.typography.body_size))
        .text_color(theme.colors.text.primary);
    for (index, row) in rows.iter().enumerate() {
        let selected = selected_parent == Some(row.selector.as_str());
        let select_id = if row.selector.is_empty() {
            "save-destination-root".to_owned()
        } else {
            format!("save-destination-{}", row.selector)
        };
        let selector = row.selector.clone();
        let action_selector = selector.clone();
        let select = on_select.clone();
        let activate = on_select.clone();
        let disclosure = if row.expandable {
            let toggle = on_toggle.clone();
            let toggle_action = on_toggle.clone();
            let selector = row.selector.clone();
            let action_selector = selector.clone();
            let toggle_id = format!("save-destination-toggle-{}", row.selector);
            Some(
                Button::new(toggle_id.clone())
                    .debug_selector(move || toggle_id.clone())
                    .key_context("SaveDestination")
                    .disabled(disabled)
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        if !disabled {
                            toggle(selector.clone(), cx);
                        }
                    })
                    .on_action(move |_: &super::ActivateSaveDestination, _, cx| {
                        cx.stop_propagation();
                        if !disabled {
                            toggle_action(action_selector.clone(), cx);
                        }
                    }),
            )
        } else {
            None
        };
        let content = if row.selector.is_empty() {
            TreeRowContent::CollectionRoot
        } else {
            TreeRowContent::Folder {
                expanded: row.expanded,
            }
        };
        let row_element = tree_row(
            theme,
            select_id.clone(),
            row.depth,
            selected,
            row.name.clone(),
            content,
            disclosure,
        )
        .map_button(|button| {
            button
                .disabled(disabled)
                .debug_selector(move || select_id.clone())
                .accessibility_label(if row.selector.is_empty() {
                    "Collection root".to_owned()
                } else {
                    format!("Folder {}", row.name)
                })
                .key_context("SaveDestination")
                .on_click(move |_, _, cx| {
                    if !disabled {
                        select(selector.clone(), cx);
                    }
                })
                .on_action(move |_: &super::ActivateSaveDestination, _, cx| {
                    cx.stop_propagation();
                    if !disabled {
                        activate(action_selector.clone(), cx);
                    }
                })
        });
        tree = tree.child(row_element.into_row(
            ("save-destination-row", index),
            format!("save-destination-row-{}", row.selector),
        ));
    }
    tree
}

pub(crate) fn tree_disclosure_width(theme: Theme) -> f32 {
    theme.metrics.icon_standard * 1.5
}

pub(crate) fn tree_level_indent(theme: Theme, depth: usize) -> f32 {
    theme.metrics.spacing_1 + depth as f32 * tree_disclosure_width(theme)
}

fn tree_hierarchy_guides(
    theme: Theme,
    depth: usize,
    selected: bool,
    row_debug_selector: &str,
) -> gpui_kit::Div {
    let mut guides = div().absolute().top(px(0.0)).bottom(px(0.0)).left(px(0.0));
    let color = if selected {
        theme.colors.selection.active_foreground.opacity(0.22)
    } else {
        theme.colors.borders.standard
    };
    for level in 0..depth {
        let guide_selector = format!("{row_debug_selector}-guide-{level}");
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
                .bg(color)
                .debug_selector(move || guide_selector.clone()),
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
