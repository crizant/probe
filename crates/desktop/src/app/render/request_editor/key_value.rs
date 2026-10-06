use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum KeyValueEditorKind {
    Path,
    Query,
    Headers,
    Form,
}

struct KeyValueRow<'a> {
    name: &'a str,
    value: &'a str,
    disabled: bool,
}

struct KeyValueRowMut<'a> {
    name: &'a mut String,
    value: &'a mut String,
    disabled: &'a mut bool,
}

impl KeyValueEditorKind {
    fn rows(self, request: &Request) -> impl Iterator<Item = KeyValueRow<'_>> {
        let mut index = 0;
        std::iter::from_fn(move || {
            let row = match self {
                Self::Path | Self::Query => {
                    let parameters = match self {
                        Self::Path => &request.path_parameters,
                        _ => &request.query_parameters,
                    };
                    let row = parameters.get(index)?;
                    KeyValueRow {
                        name: &row.name,
                        value: &row.value,
                        disabled: row.disabled,
                    }
                }
                Self::Headers => {
                    let row = request.headers.get(index)?;
                    KeyValueRow {
                        name: &row.name,
                        value: &row.value,
                        disabled: row.disabled,
                    }
                }
                Self::Form => {
                    let Some(RequestBody::Single(Body::FormUrlEncoded(fields))) =
                        request.http_body()
                    else {
                        return None;
                    };
                    let row = fields.get(index)?;
                    KeyValueRow {
                        name: &row.name,
                        value: &row.value,
                        disabled: row.disabled,
                    }
                }
            };
            index += 1;
            Some(row)
        })
    }

    fn row_mut(self, request: &mut Request, index: usize) -> Option<KeyValueRowMut<'_>> {
        Some(match self {
            Self::Path | Self::Query => {
                let parameters = match self {
                    Self::Path => &mut request.path_parameters,
                    _ => &mut request.query_parameters,
                };
                let row = parameters.get_mut(index)?;
                KeyValueRowMut {
                    name: &mut row.name,
                    value: &mut row.value,
                    disabled: &mut row.disabled,
                }
            }
            Self::Headers => {
                let row = request.headers.get_mut(index)?;
                KeyValueRowMut {
                    name: &mut row.name,
                    value: &mut row.value,
                    disabled: &mut row.disabled,
                }
            }
            Self::Form => {
                let Some(RequestBody::Single(Body::FormUrlEncoded(fields))) =
                    request.http_body_mut()
                else {
                    return None;
                };
                let row = fields.get_mut(index)?;
                KeyValueRowMut {
                    name: &mut row.name,
                    value: &mut row.value,
                    disabled: &mut row.disabled,
                }
            }
        })
    }

    fn rename(self, request: &mut Request, index: usize, name: &str) {
        if self == Self::Path {
            rename_path_parameter_at(request, index, name);
        } else if let Some(row) = self.row_mut(request, index) {
            *row.name = name.to_owned();
        }
    }

    fn remove(self, request: &mut Request, index: usize) {
        match self {
            Self::Path => {
                remove_path_parameter_at(request, index);
            }
            Self::Query => remove_at(&mut request.query_parameters, index),
            Self::Headers => remove_at(&mut request.headers, index),
            Self::Form => {
                if let Some(RequestBody::Single(Body::FormUrlEncoded(fields))) =
                    request.http_body_mut()
                {
                    remove_at(fields, index);
                }
            }
        }
    }

    fn add(self, request: &mut Request) {
        match self {
            Self::Path => add_path_parameter(request),
            Self::Query => request.query_parameters.push(QueryParameter {
                name: String::new(),
                value: String::new(),
                disabled: false,
            }),
            Self::Headers => request.headers.push(Header {
                name: String::new(),
                value: String::new(),
                disabled: false,
            }),
            Self::Form => {
                if let Some(RequestBody::Single(Body::FormUrlEncoded(fields))) =
                    request.http_body_mut()
                {
                    fields.push(FormField {
                        name: String::new(),
                        value: String::new(),
                        disabled: false,
                    });
                }
            }
        }
    }

    const fn name_id(self) -> &'static str {
        match self {
            Self::Path => "path-name",
            Self::Query => "query-name",
            Self::Headers => "header-name",
            Self::Form => "form-field-name",
        }
    }

    const fn name_placeholder(self) -> &'static str {
        match self {
            Self::Path | Self::Query => "Parameter",
            Self::Headers => "Header",
            Self::Form => "Field",
        }
    }

    const fn value_id(self) -> &'static str {
        match self {
            Self::Path => "path-value",
            Self::Query => "query-value",
            Self::Headers => "header-value",
            Self::Form => "form-field-value",
        }
    }

    const fn enabled_id(self) -> &'static str {
        match self {
            Self::Path => "path-enabled",
            Self::Query => "query-enabled",
            Self::Headers => "header-enabled",
            Self::Form => "form-field-enabled",
        }
    }

    const fn remove_id(self) -> &'static str {
        match self {
            Self::Path => "remove-path",
            Self::Query => "remove-query",
            Self::Headers => "remove-header",
            Self::Form => "remove-form-field",
        }
    }

    const fn add_id(self) -> &'static str {
        match self {
            Self::Path => "add-path-parameter",
            Self::Query => "add-query-parameter",
            Self::Headers => "add-header",
            Self::Form => "add-form-field",
        }
    }

    const fn enable_label(self) -> &'static str {
        match self {
            Self::Path => "Enable path parameter",
            Self::Query => "Enable query parameter",
            Self::Headers => "Enable header",
            Self::Form => "Enable form field",
        }
    }

    const fn remove_label(self) -> &'static str {
        match self {
            Self::Path => "Remove path parameter",
            Self::Query => "Remove query parameter",
            Self::Headers => "Remove header",
            Self::Form => "Remove form field",
        }
    }

    const fn add_label(self) -> &'static str {
        match self {
            Self::Path => "Add path parameter",
            Self::Query => "Add query parameter",
            Self::Headers => "Add header",
            Self::Form => "Add field",
        }
    }
}

fn remove_at<T>(rows: &mut Vec<T>, index: usize) {
    if index < rows.len() {
        rows.remove(index);
    }
}

impl ProbeApp {
    pub(super) fn render_key_value_editor(
        &self,
        key: RequestKey,
        request: &Request,
        kind: KeyValueEditorKind,
        theme: Theme,
        list_scroll: Option<&components::ListScroll>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut rows = div().flex().flex_col().gap(px(theme.metrics.spacing_2));
        for (index, row) in kind.rows(request).enumerate() {
            let name_view = cx.weak_entity();
            let value_view = cx.weak_entity();
            let enabled_view = cx.weak_entity();
            let remove_view = cx.weak_entity();
            rows = rows.child(
                components::editor_key_value_row(theme)
                    .when(
                        index == 0
                            && matches!(kind, KeyValueEditorKind::Path | KeyValueEditorKind::Query),
                        |row| row.debug_selector(move || format!("{}-row", kind.name_id())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .when(index == 0 && kind == KeyValueEditorKind::Headers, |cell| {
                                cell.debug_selector(|| "header-name-field".into())
                            })
                            .child(
                                components::variable_text_input(
                                    theme,
                                    (kind.name_id(), index),
                                    row.name.to_owned(),
                                    kind.name_placeholder(),
                                    self.variable_context(cx),
                                    move |value, _, input_cx| {
                                        let _ = name_view.update(input_cx, |view, cx| {
                                            view.edit_request(
                                                key,
                                                |request| {
                                                    kind.rename(request, index, &value);
                                                },
                                                cx,
                                            );
                                        });
                                    },
                                )
                                .list_scroll(list_scroll),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .when(kind == KeyValueEditorKind::Headers, |cell| {
                                cell.debug_selector(move || {
                                    if index == 0 {
                                        "header-value-field".into()
                                    } else {
                                        format!("header-value-field-{index}")
                                    }
                                })
                            })
                            .child(
                                components::variable_text_input(
                                    theme,
                                    (kind.value_id(), index),
                                    row.value.to_owned(),
                                    "Value",
                                    self.variable_context(cx),
                                    move |value, _, input_cx| {
                                        let _ = value_view.update(input_cx, |view, cx| {
                                            view.edit_request(
                                                key,
                                                |request| {
                                                    if let Some(row) = kind.row_mut(request, index)
                                                    {
                                                        *row.value = value.to_string();
                                                    }
                                                },
                                                cx,
                                            );
                                        });
                                    },
                                )
                                .list_scroll(list_scroll),
                            ),
                    )
                    .child(components::switch(
                        theme,
                        (kind.enabled_id(), index),
                        kind.enable_label(),
                        !row.disabled,
                        false,
                        move |enabled, _, cx| {
                            let _ = enabled_view.update(cx, |view, cx| {
                                view.edit_request(
                                    key,
                                    |request| {
                                        if let Some(row) = kind.row_mut(request, index) {
                                            *row.disabled = !enabled;
                                        }
                                    },
                                    cx,
                                );
                            });
                        },
                    ))
                    .child(components::remove_row_button(
                        theme,
                        (kind.remove_id(), index),
                        kind.remove_label(),
                        move |_, window, cx| {
                            let _ = remove_view.update(cx, |view, cx| {
                                view.edit_request(
                                    key,
                                    |request| {
                                        kind.remove(request, index);
                                    },
                                    cx,
                                );
                                view.focus_handle.focus(window, cx);
                            });
                        },
                    )),
            );
        }
        let add_view = cx.weak_entity();
        rows.child(components::editor_add_button(
            theme,
            kind.add_id(),
            kind.add_label(),
            move |_, _, cx| {
                let _ = add_view.update(cx, |view, cx| {
                    view.edit_request(
                        key,
                        |request| {
                            kind.add(request);
                        },
                        cx,
                    );
                });
            },
        ))
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use probe_core::{RawBody, RequestKind};

    #[test]
    fn header_and_form_adapters_use_their_native_fields() {
        for kind in [KeyValueEditorKind::Headers, KeyValueEditorKind::Form] {
            let mut request = Request {
                kind: RequestKind::Http {
                    body: Some(RequestBody::Single(Body::FormUrlEncoded(Vec::new()))),
                },
                ..Request::default()
            };
            let empty = request.clone();
            kind.add(&mut request);
            kind.rename(&mut request, 0, "name");
            let row = kind.row_mut(&mut request, 0).unwrap();
            *row.value = "value".into();
            *row.disabled = true;

            let Some(RequestBody::Single(Body::FormUrlEncoded(fields))) = request.http_body()
            else {
                panic!("adapter must preserve the body kind");
            };
            assert_eq!(
                (request.headers.len(), fields.len()),
                if kind == KeyValueEditorKind::Headers {
                    (1, 0)
                } else {
                    (0, 1)
                },
            );
            let rows: Vec<_> = kind
                .rows(&request)
                .map(|row| (row.name, row.value, row.disabled))
                .collect();
            assert_eq!(rows, [("name", "value", true)]);

            let before_stale_edit = request.clone();
            kind.rename(&mut request, 1, "stale");
            assert!(kind.row_mut(&mut request, 1).is_none());
            kind.remove(&mut request, 1);
            assert_eq!(request, before_stale_edit);
            kind.remove(&mut request, 0);
            assert_eq!(request, empty);
        }
    }

    #[test]
    fn form_callbacks_do_not_replace_a_different_body_kind() {
        let kind = KeyValueEditorKind::Form;
        for body_kind in [
            RequestKind::default(),
            RequestKind::Http {
                body: Some(RequestBody::Single(Body::Raw(RawBody {
                    kind: RawBodyKind::Json,
                    data: r#"{"keep":true}"#.into(),
                }))),
            },
            RequestKind::Http {
                body: Some(RequestBody::Single(Body::Multipart(Vec::new()))),
            },
            RequestKind::Graphql { body: None },
        ] {
            let mut request = Request {
                kind: body_kind,
                ..Request::default()
            };
            let original = request.clone();
            assert_eq!(kind.rows(&request).count(), 0);
            kind.rename(&mut request, 0, "stale");
            assert!(kind.row_mut(&mut request, 0).is_none());
            kind.remove(&mut request, 0);
            kind.add(&mut request);
            assert_eq!(request, original);
        }
    }
}
