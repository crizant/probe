use gpui::{IntoElement as _, ParentElement as _, Styled as _, div, px};

use crate::{
    response_inspector::InspectSelection, response_viewer::PreparedDocument, theme::Theme,
};

pub(crate) fn request_protocol_label(protocol: &probe_core::RequestProtocol) -> &'static str {
    match protocol {
        probe_core::RequestProtocol::Http => "HTTP",
        probe_core::RequestProtocol::Graphql(_) => "GQL",
    }
}

pub(crate) fn request_protocol_color(
    theme: Theme,
    protocol: &probe_core::RequestProtocol,
) -> gpui::Rgba {
    match protocol {
        probe_core::RequestProtocol::Http => theme.colors.protocols.http,
        probe_core::RequestProtocol::Graphql(_) => theme.colors.protocols.graphql,
    }
}

pub(crate) fn request_navigation_label(
    protocol: &probe_core::RequestProtocol,
    method: &str,
) -> String {
    match protocol {
        probe_core::RequestProtocol::Http => super::tree::tree_method_label(method).to_owned(),
        probe_core::RequestProtocol::Graphql(_) => request_protocol_label(protocol).to_owned(),
    }
}

pub(crate) fn request_navigation_color(
    theme: Theme,
    protocol: &probe_core::RequestProtocol,
    method: &str,
) -> gpui::Rgba {
    match protocol {
        probe_core::RequestProtocol::Http => theme.method_color(method),
        probe_core::RequestProtocol::Graphql(_) => request_protocol_color(theme, protocol),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InspectListRow {
    Group { label: &'static str, count: usize },
    Item { selection: InspectSelection },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PrettyRevealState {
    pub(crate) selection: InspectSelection,
    pub(crate) scroll_pending: bool,
}

pub(crate) fn response_status_color(theme: Theme, status: u16) -> gpui::Rgba {
    match status {
        100..=199 => theme.colors.responses.informational,
        200..=299 => theme.colors.responses.success,
        300..=399 => theme.colors.responses.redirect,
        400..=499 => theme.colors.responses.client_error,
        500..=599 => theme.colors.responses.server_error,
        _ => theme.colors.text.muted,
    }
}

pub(crate) fn inspect_list_rows(document: &PreparedDocument) -> Vec<InspectListRow> {
    let mut rows = Vec::with_capacity(document.inspection.count() + 2);
    if !document.inspection.jwts.is_empty() {
        rows.push(InspectListRow::Group {
            label: "JWT",
            count: document.inspection.jwts.len(),
        });
        rows.extend(
            (0..document.inspection.jwts.len()).map(|index| InspectListRow::Item {
                selection: InspectSelection::Jwt(index),
            }),
        );
    }
    if !document.inspection.timestamps.is_empty() {
        rows.push(InspectListRow::Group {
            label: "Timestamps",
            count: document.inspection.timestamps.len(),
        });
        rows.extend(
            (0..document.inspection.timestamps.len()).map(|index| InspectListRow::Item {
                selection: InspectSelection::Timestamp(index),
            }),
        );
    }
    rows
}

pub(crate) fn inspect_row_label(
    document: &PreparedDocument,
    selection: InspectSelection,
) -> String {
    match selection {
        InspectSelection::Jwt(index) => document
            .inspection
            .jwts
            .get(index)
            .map(|finding| finding.path.clone())
            .unwrap_or_else(|| "JWT".to_owned()),
        InspectSelection::Timestamp(index) => document
            .inspection
            .timestamps
            .get(index)
            .map(|finding| finding.path.clone())
            .unwrap_or_else(|| "Timestamp".to_owned()),
    }
}

pub(crate) fn inspect_row_index(
    rows: &[InspectListRow],
    selection: InspectSelection,
) -> Option<usize> {
    rows.iter().position(|row| {
        matches!(
            row,
            InspectListRow::Item {
                selection: row_selection
            } if *row_selection == selection
        )
    })
}

pub(crate) fn placeholder_message(theme: Theme, message: &str) -> gpui::AnyElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .p(px(theme.metrics.spacing_3))
        .text_color(theme.colors.text.muted)
        .child(message.to_owned())
        .into_any_element()
}

pub(crate) fn request_method_options(
    theme: Theme,
    active_method: &str,
) -> Vec<(String, String, Option<gpui::Rgba>)> {
    let mut methods = vec!["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
    if !methods.contains(&active_method) {
        methods.push(active_method);
    }
    methods
        .into_iter()
        .map(|method| {
            (
                method.to_owned(),
                method.to_owned(),
                Some(theme.method_color(method)),
            )
        })
        .collect()
}

pub(crate) struct ShellSelectors {
    pub(crate) tab_selectors: Vec<String>,
    pub(crate) active_selector: Option<String>,
    pub(crate) folder_selectors: Vec<String>,
    pub(crate) selected: Option<(probe_opencollection::ItemKind, String)>,
}

#[cfg(test)]
mod tests {
    use probe_core::RequestProtocol;

    use super::{request_navigation_label, request_protocol_label};

    #[test]
    fn request_labels_distinguish_http_methods_from_graphql() {
        assert_eq!(request_protocol_label(&RequestProtocol::Http), "HTTP");
        assert_eq!(
            request_protocol_label(&RequestProtocol::Graphql(None)),
            "GQL"
        );
        assert_eq!(
            request_navigation_label(&RequestProtocol::Http, "POST"),
            "POST"
        );
        assert_eq!(
            request_navigation_label(&RequestProtocol::Graphql(None), "POST"),
            "GQL"
        );
    }
}
