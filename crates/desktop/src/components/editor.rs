use super::*;

mod auto_edit;
mod core;
mod response;
mod search;
#[cfg(test)]
mod tests;
mod variables;

#[cfg(not(test))]
use core::ProbeEditor;
pub(crate) use core::editor_paint_style;
#[cfg(test)]
pub(super) use core::{ProbeEditor, editor_value_needs_refresh};
pub(crate) use response::{
    BodySyntax, body_text_input, documentation_text_input, response_body_input,
    response_headers_input, response_inspector_input,
};
#[cfg(not(test))]
use search::body_text_highlights;
use search::{
    EditorSearchState, editor_search_card_overlay, response_search_highlight_overlay,
    search_match_decoration, text_decoration,
};
#[cfg(test)]
pub(super) use search::{
    body_text_highlights, normalize_search_char_bounds, search_fallback_char_size,
    search_match_bounds, search_match_char_ranges,
};
#[cfg(test)]
pub(super) use variables::SecretTooltipState;
pub(crate) use variables::single_line;
use variables::variable_editor_overlay;
#[cfg(test)]
pub(super) use variables::{
    PlaceholderTone, ReferenceKind, VariableHighlightElement, VariableHighlightPalette,
    placeholder_tone, reference_status, variable_highlight_palette, variable_highlight_runs,
    variable_ranges, variable_span_layout, variable_tooltip_presentation,
};
pub(super) use variables::{
    VariableTooltipPresentation, input_variable_ranges, variable_input_overlay,
};
#[cfg(not(test))]
use variables::{placeholder_tone, variable_highlight_palette, variable_ranges};
