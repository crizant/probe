use super::*;
use crate::websocket::{DisconnectSeverity, disconnect_severity_color};

static CHEVRON_DOWN_SVG: LazyLock<Vec<u8>> =
    LazyLock::new(|| icon_svg_bytes(icondata::LuChevronDown));
static CHEVRON_UP_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuChevronUp));
pub(super) static CHEVRON_RIGHT_SVG: LazyLock<Vec<u8>> =
    LazyLock::new(|| icon_svg_bytes(icondata::LuChevronRight));
pub(super) static CHECK_SVG: LazyLock<Vec<u8>> =
    LazyLock::new(|| icon_svg_bytes(icondata::LuCheck));
static FOLDER_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuFolder));
static LIBRARY_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuLibrary));
static FOLDER_OPEN_SVG: LazyLock<Vec<u8>> =
    LazyLock::new(|| icon_svg_bytes(icondata::LuFolderOpen));
static PLUS_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuPlus));
pub(super) static SEARCH_SVG: LazyLock<Vec<u8>> =
    LazyLock::new(|| icon_svg_bytes(icondata::LuSearch));
static SAVE_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuSave));
static DOWNLOAD_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuDownload));
static CLOSE_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuX));
static CLEAR_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuEraser));
static TRASH_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuTrash2));
static EDIT_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuPencil));
static COPY_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuCopy));
static LOCATE_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuLocateFixed));
static SIDEBAR_COLLAPSE_SVG: LazyLock<Vec<u8>> =
    LazyLock::new(|| icon_svg_bytes(icondata::LuPanelLeftClose));
static SIDEBAR_EXPAND_SVG: LazyLock<Vec<u8>> =
    LazyLock::new(|| icon_svg_bytes(icondata::LuPanelLeftOpen));
static HOME_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuHouse));
static EYE_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuEye));
static EYE_OFF_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuEyeOff));
static LOCK_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuLock));
static SEVERITY_DIAMOND_SVG: LazyLock<Vec<u8>> =
    LazyLock::new(|| icon_svg_bytes(icondata::LuDiamond));
static SEVERITY_TRIANGLE_SVG: LazyLock<Vec<u8>> =
    LazyLock::new(|| icon_svg_bytes(icondata::LuTriangle));
static SEVERITY_OCTAGON_SVG: LazyLock<Vec<u8>> =
    LazyLock::new(|| icon_svg_bytes(icondata::LuOctagon));
static SEVERITY_MINUS_SVG: LazyLock<Vec<u8>> = LazyLock::new(|| icon_svg_bytes(icondata::LuMinus));

fn icon_svg_bytes(icon: icondata::Icon) -> Vec<u8> {
    format!(
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{}" fill="{}" "#,
            r#"stroke="{}" stroke-width="{}" stroke-linecap="{}" stroke-linejoin="{}">"#,
            "{}",
            "</svg>"
        ),
        icon.view_box.unwrap_or("0 0 24 24"),
        icon.fill.unwrap_or("none"),
        icon.stroke.unwrap_or("currentColor"),
        icon.stroke_width.unwrap_or("2"),
        icon.stroke_linecap.unwrap_or("round"),
        icon.stroke_linejoin.unwrap_or("round"),
        icon.data,
    )
    .into_bytes()
}

pub(super) fn library_icon(
    cache_key: &'static str,
    data: &'static LazyLock<Vec<u8>>,
    size: f32,
) -> gpui_kit::Div {
    svg_icon(cache_key, data.as_slice(), size, size)
}

fn svg_icon(
    cache_key: &'static str,
    data: &'static [u8],
    width: f32,
    height: f32,
) -> gpui_kit::Div {
    let width = px(width);
    let height = px(height);
    div()
        .flex_none()
        .w(width)
        .h(height)
        .flex()
        .items_center()
        .justify_center()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, cx| {
                    let _ = window.paint_svg(
                        bounds,
                        SharedString::from(cache_key),
                        Some(data),
                        TransformationMatrix::default(),
                        window.text_style().color,
                        cx,
                    );
                },
            )
            .w(width)
            .h(height),
        )
}

/// Presentation data for a request icon, without retaining its request body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RequestIcon {
    Http { method: Option<String> },
    Graphql,
    WebSocket,
}

impl RequestIcon {
    pub(crate) fn from_request(kind: &probe_core::RequestKind, method: Option<&str>) -> Self {
        match kind.protocol() {
            probe_core::RequestProtocol::Http => Self::Http {
                method: method.map(str::to_uppercase),
            },
            probe_core::RequestProtocol::Graphql => Self::Graphql,
            probe_core::RequestProtocol::WebSocket => Self::WebSocket,
        }
    }

    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Http { method } => method.as_deref().unwrap_or("HTTP"),
            Self::Graphql => "GraphQL",
            Self::WebSocket => "WebSocket",
        }
    }

    pub(crate) fn custom_method(&self) -> Option<&str> {
        match self {
            Self::Http {
                method: Some(method),
            } if !self.asset().2 => Some(method),
            _ => None,
        }
    }

    fn asset(&self) -> (&'static str, &'static [u8], bool) {
        match self {
            RequestIcon::Graphql => (
                "probe-request-graphql",
                include_bytes!("../../assets/icons/requests/graphql.svg"),
                false,
            ),
            RequestIcon::WebSocket => (
                "probe-request-websocket",
                include_bytes!("../../assets/icons/requests/websocket.svg"),
                false,
            ),
            RequestIcon::Http { method } => match method.as_deref().unwrap_or("HTTP") {
                "GET" => (
                    "probe-request-get",
                    include_bytes!("../../assets/icons/requests/get.svg"),
                    true,
                ),
                "POST" => (
                    "probe-request-post",
                    include_bytes!("../../assets/icons/requests/post.svg"),
                    true,
                ),
                "PUT" => (
                    "probe-request-put",
                    include_bytes!("../../assets/icons/requests/put.svg"),
                    true,
                ),
                "PATCH" => (
                    "probe-request-patch",
                    include_bytes!("../../assets/icons/requests/patch.svg"),
                    true,
                ),
                "DELETE" => (
                    "probe-request-delete",
                    include_bytes!("../../assets/icons/requests/delete.svg"),
                    true,
                ),
                "HEAD" => (
                    "probe-request-head",
                    include_bytes!("../../assets/icons/requests/head.svg"),
                    true,
                ),
                "OPTIONS" => (
                    "probe-request-options",
                    include_bytes!("../../assets/icons/requests/options.svg"),
                    true,
                ),
                "CONNECT" => (
                    "probe-request-connect",
                    include_bytes!("../../assets/icons/requests/connect.svg"),
                    true,
                ),
                "TRACE" => (
                    "probe-request-trace",
                    include_bytes!("../../assets/icons/requests/trace.svg"),
                    true,
                ),
                _ => (
                    "probe-request-http",
                    include_bytes!("../../assets/icons/requests/http.svg"),
                    false,
                ),
            },
        }
    }
}

pub(crate) fn request_icon(theme: Theme, icon: &RequestIcon) -> gpui_kit::Div {
    let (cache_key, data, is_method) = icon.asset();
    let height = theme.metrics.icon_standard;
    let width = height * 1.5;
    let color = match icon {
        RequestIcon::Graphql => theme.colors.protocols.graphql,
        RequestIcon::Http { .. } if is_method => theme.method_color(icon.label()),
        RequestIcon::Http { .. } | RequestIcon::WebSocket => theme.colors.protocols.http,
    };
    tree_icon_slot(
        theme,
        svg_icon(
            cache_key,
            data,
            if is_method { width } else { height },
            height,
        ),
    )
    .text_color(color)
}

pub(crate) fn tree_icon_slot(theme: Theme, icon: impl IntoElement) -> gpui_kit::Div {
    let height = theme.metrics.icon_standard * 1.25;
    div()
        .w(px(height))
        .h(px(height))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(icon)
}

pub(crate) fn tree_disclosure_icon(theme: Theme, expanded: bool) -> gpui_kit::Div {
    let (name, svg) = if expanded {
        ("lucide-chevron-down", &CHEVRON_DOWN_SVG)
    } else {
        ("lucide-chevron-right", &CHEVRON_RIGHT_SVG)
    };
    library_icon(name, svg, theme.metrics.icon_small).text_color(theme.colors.text.muted)
}

pub(crate) fn chevron_icon(theme: Theme, expanded: bool) -> gpui_kit::Div {
    let icon = if expanded {
        library_icon(
            "lucide-chevron-up",
            &CHEVRON_UP_SVG,
            theme.metrics.icon_small,
        )
    } else {
        library_icon(
            "lucide-chevron-down",
            &CHEVRON_DOWN_SVG,
            theme.metrics.icon_small,
        )
    };
    icon.text_color(theme.colors.text.muted)
}

pub(crate) fn reorder_icon(theme: Theme, up: bool) -> gpui_kit::Div {
    let (name, svg) = if up {
        ("lucide-chevron-up", &CHEVRON_UP_SVG)
    } else {
        ("lucide-chevron-down", &CHEVRON_DOWN_SVG)
    };
    library_icon(name, svg, 14.0).text_color(theme.colors.text.secondary)
}

pub(crate) fn eye_icon(theme: Theme, revealed: bool) -> gpui_kit::Div {
    let (key, data) = if revealed {
        ("lucide-eye-off", &EYE_OFF_SVG)
    } else {
        ("lucide-eye", &EYE_SVG)
    };
    library_icon(key, data, theme.metrics.icon_standard).text_color(theme.colors.text.secondary)
}

pub(crate) fn lock_icon(theme: Theme) -> gpui_kit::Div {
    library_icon("lucide-lock", &LOCK_SVG, theme.metrics.icon_small)
        .text_color(theme.colors.text.muted)
}

fn tree_item_icon_color(theme: Theme, selected: bool) -> gpui_kit::Rgba {
    if selected {
        theme.colors.selection.active_foreground
    } else {
        theme.colors.text.muted
    }
}

fn folder_icon(theme: Theme, expanded: bool, selected: bool, size: f32) -> gpui_kit::Div {
    let icon = if expanded {
        library_icon("lucide-folder-open", &FOLDER_OPEN_SVG, size)
    } else {
        library_icon("lucide-folder", &FOLDER_SVG, size)
    };
    icon.text_color(tree_item_icon_color(theme, selected))
}

pub(crate) fn tree_folder_icon(theme: Theme, expanded: bool, selected: bool) -> gpui_kit::Div {
    folder_icon(theme, expanded, selected, theme.metrics.icon_standard)
}

pub(crate) fn collection_icon(theme: Theme) -> gpui_kit::Div {
    library_icon("lucide-library", &LIBRARY_SVG, theme.metrics.icon_standard)
}

pub(crate) fn menu_folder_icon(theme: Theme) -> gpui_kit::Div {
    folder_icon(theme, false, false, 14.0)
}

pub(crate) fn plus_icon(_theme: Theme) -> gpui_kit::Div {
    library_icon("lucide-plus", &PLUS_SVG, 14.0)
}

pub(crate) fn hover_fill(color: gpui_kit::Rgba) -> gpui_kit::Rgba {
    let mut hover: Hsla = color.into();
    hover.l = if hover.l < 0.5 {
        (hover.l + 0.08).min(1.0)
    } else {
        (hover.l * 0.92).max(0.0)
    };
    hover.into()
}

pub(crate) fn add_menu_button(theme: Theme, open: bool, enabled: bool) -> Button {
    add_menu_button_with_id(
        theme,
        "tree-add-menu-trigger",
        "Add request or folder",
        open,
        enabled,
    )
}

pub(crate) fn add_menu_button_with_id(
    theme: Theme,
    id: &'static str,
    label: &'static str,
    open: bool,
    enabled: bool,
) -> Button {
    icon_button_base(
        theme,
        id,
        id,
        label,
        enabled,
        open && enabled,
        hover_fill(theme.colors.surfaces.window),
    )
    .child(
        library_icon("lucide-plus", &PLUS_SVG, theme.metrics.icon_standard).text_color(
            if enabled {
                theme.colors.text.primary
            } else {
                theme.colors.actions.disabled_foreground
            },
        ),
    )
}

pub(crate) fn save_icon(theme: Theme) -> gpui_kit::Div {
    library_icon("lucide-save", &SAVE_SVG, theme.metrics.icon_standard)
        .text_color(theme.colors.text.primary)
}

pub(crate) fn download_icon(theme: Theme) -> gpui_kit::Div {
    library_icon(
        "lucide-download",
        &DOWNLOAD_SVG,
        theme.metrics.icon_standard,
    )
    .text_color(theme.colors.text.primary)
}

pub(crate) fn close_icon(theme: Theme) -> gpui_kit::Div {
    library_icon("lucide-x", &CLOSE_SVG, theme.metrics.icon_standard)
        .text_color(theme.colors.text.secondary)
}

/// Uses the same Lucide status marks as the connection status surfaces.
pub(crate) fn assertion_status_icon(
    theme: Theme,
    outcome: &probe_application::AssertionOutcome,
) -> gpui_kit::Div {
    use probe_application::AssertionOutcome;
    let (name, svg) = match outcome {
        AssertionOutcome::Passed => ("assertion-passed", &CHECK_SVG),
        AssertionOutcome::Failed => ("assertion-failed", &CLOSE_SVG),
        AssertionOutcome::Error(_) => ("assertion-error", &SEVERITY_TRIANGLE_SVG),
        AssertionOutcome::Disabled => ("assertion-disabled", &SEVERITY_MINUS_SVG),
    };
    library_icon(name, svg, theme.metrics.icon_standard)
}

pub(crate) fn disconnect_severity_mark(
    theme: Theme,
    severity: DisconnectSeverity,
) -> gpui_kit::Div {
    let (name, svg) = match severity {
        DisconnectSeverity::Success => ("disconnect-severity-success", &CHECK_SVG),
        DisconnectSeverity::Temporary => ("disconnect-severity-temporary", &SEVERITY_DIAMOND_SVG),
        DisconnectSeverity::ClientError => ("disconnect-severity-client", &SEVERITY_TRIANGLE_SVG),
        DisconnectSeverity::Failure => ("disconnect-severity-failure", &SEVERITY_OCTAGON_SVG),
        DisconnectSeverity::Neutral => ("disconnect-severity-neutral", &SEVERITY_MINUS_SVG),
    };
    library_icon(name, svg, theme.metrics.icon_small)
        .text_color(disconnect_severity_color(theme, severity))
}

pub(crate) fn edit_icon(theme: Theme) -> gpui_kit::Div {
    library_icon("lucide-pencil", &EDIT_SVG, 14.0).text_color(theme.colors.text.secondary)
}

pub(crate) fn field_copy_icon(theme: Theme) -> gpui_kit::Div {
    library_icon("lucide-copy", &COPY_SVG, 14.0).text_color(theme.colors.text.secondary)
}

pub(crate) fn copy_icon(theme: Theme) -> gpui_kit::Div {
    library_icon("lucide-copy", &COPY_SVG, theme.metrics.icon_standard)
        .text_color(theme.colors.text.secondary)
}

pub(crate) fn locate_icon(theme: Theme) -> gpui_kit::Div {
    library_icon(
        "lucide-locate-fixed",
        &LOCATE_SVG,
        theme.metrics.icon_standard,
    )
    .text_color(theme.colors.text.secondary)
}

fn sidebar_icon(theme: Theme, collapsed: bool) -> gpui_kit::Div {
    if collapsed {
        library_icon(
            "lucide-panel-left-open",
            &SIDEBAR_EXPAND_SVG,
            theme.metrics.icon_standard,
        )
    } else {
        library_icon(
            "lucide-panel-left-close",
            &SIDEBAR_COLLAPSE_SVG,
            theme.metrics.icon_standard,
        )
    }
    .text_color(theme.colors.text.secondary)
}

pub(crate) fn sidebar_toggle(
    theme: Theme,
    collapsed: bool,
    on_toggle: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = if collapsed {
        "Show sidebar"
    } else {
        "Hide sidebar"
    };
    icon_button_base(
        theme,
        "sidebar-toggle",
        "sidebar-toggle",
        label,
        true,
        false,
        theme.colors.surfaces.sidebar,
    )
    .child(sidebar_icon(theme, collapsed))
    .on_click(move |_, window, cx| on_toggle(window, cx))
}

pub(crate) fn home_button(
    theme: Theme,
    enabled: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    icon_button_base(
        theme,
        "home-button",
        "home-button",
        "Home",
        enabled,
        false,
        theme.colors.surfaces.sidebar,
    )
    .child(
        library_icon("lucide-house", &HOME_SVG, theme.metrics.icon_standard).text_color(
            if enabled {
                theme.colors.text.secondary
            } else {
                theme.colors.actions.disabled_foreground
            },
        ),
    )
    .on_click(move |_, window, cx| on_click(window, cx))
}

fn icon_button_base(
    theme: Theme,
    id: &'static str,
    debug_selector: &'static str,
    accessibility_label: &'static str,
    enabled: bool,
    selected: bool,
    active_background: gpui_kit::Rgba,
) -> Button {
    Button::new(id)
        .hint(theme, accessibility_label)
        .accessibility_label(accessibility_label)
        .debug_selector(move || debug_selector.into())
        .selected(selected)
        .disabled(!enabled)
        .size(px(theme.metrics.control_height))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme.metrics.radius_small))
        .when(!enabled, |button| button.bg(theme.colors.actions.disabled))
        .when(enabled, |button| {
            button
                .hover(move |button| button.bg(active_background))
                .focus_visible(move |button| button.bg(active_background))
                .styles(move |styles| styles.selected(move |button| button.bg(active_background)))
        })
}

pub(super) fn folder_open_icon(color: gpui_kit::Rgba) -> gpui_kit::Div {
    library_icon("lucide-folder-open", &FOLDER_OPEN_SVG, 14.0).text_color(color)
}

pub(crate) fn trash_icon(color: gpui_kit::Rgba) -> gpui_kit::Div {
    library_icon("lucide-trash-2", &TRASH_SVG, 14.0).text_color(color)
}

pub(crate) fn clear_icon(theme: Theme) -> gpui_kit::Div {
    library_icon("lucide-eraser", &CLEAR_SVG, theme.metrics.icon_standard)
        .text_color(theme.colors.text.secondary)
}
