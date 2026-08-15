use gpui::{
    App, AppContext as _, Bounds, Context, FontWeight, IntoElement, ParentElement as _, Render,
    Styled as _, TitlebarOptions, Window, WindowBounds, WindowOptions, div, px, relative, size,
};
use probe_core::{Collection, Workspace};

use crate::{components, theme::Theme};

pub struct ProbeApp {
    workspace: Workspace,
    compact_mode: bool,
    activation_count: usize,
}

impl ProbeApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe_window_appearance(window, |_, window, _| window.refresh())
            .detach();

        Self {
            // The desktop adapter owns presentation state, while its workspace comes
            // directly from the same domain type used by the CLI.
            workspace: Workspace::from_collection(Collection::default()),
            compact_mode: false,
            activation_count: 0,
        }
    }
}

impl Render for ProbeApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window_appearance(window.appearance());
        let button_view = cx.weak_entity();
        let switch_view = cx.weak_entity();
        let status = if self.activation_count == 0 {
            "Keyboard and pointer ready".to_owned()
        } else {
            format!("Primitive activated {} time(s)", self.activation_count)
        };

        div()
            .size_full()
            .bg(theme.colors.surfaces.window)
            .text_color(theme.colors.text.primary)
            .font_family(theme.typography.interface_family)
            .text_size(px(theme.typography.body_size))
            .line_height(relative(theme.typography.body_line_height))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(52.0))
                    .w_full()
                    .px(px(theme.metrics.spacing_4))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(theme.colors.surfaces.raised)
                    .border_b_1()
                    .border_color(theme.colors.borders.subtle)
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Probe"),
                    )
                    .child(
                        div()
                            .text_size(px(theme.typography.caption_size))
                            .text_color(theme.colors.text.muted)
                            .child("Native desktop foundation"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(if self.compact_mode { 20.0 } else { 36.0 }))
                    .child(
                        div()
                            .w(px(520.0))
                            .p(px(if self.compact_mode { 20.0 } else { 28.0 }))
                            .flex()
                            .flex_col()
                            .gap(px(theme.metrics.spacing_4))
                            .bg(theme.colors.surfaces.editor)
                            .border_1()
                            .border_color(theme.colors.borders.standard)
                            .rounded(px(theme.metrics.radius_large))
                            .child(
                                div()
                                    .text_size(px(theme.typography.title_size))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Desktop foundation is ready"),
                            )
                            .child(
                                div()
                                    .text_color(theme.colors.text.secondary)
                                    .child(format!(
                                        "The shared core is connected ({} requests). The window follows system appearance and every visible color comes from semantic tokens.",
                                        self.workspace.request_count()
                                    )),
                            )
                            .child(
                                div()
                                    .h(px(1.0))
                                    .w_full()
                                    .bg(theme.colors.borders.subtle),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(theme.metrics.spacing_1))
                                            .child("Compact spacing")
                                            .child(
                                                div()
                                                    .text_size(px(theme.typography.caption_size))
                                                    .text_color(theme.colors.text.muted)
                                                    .child("A keyboard-accessible base-gpui switch"),
                                            ),
                                    )
                                    .child(components::switch(
                                        theme,
                                        "compact-spacing",
                                        "Compact spacing",
                                        self.compact_mode,
                                        move |checked, _, cx| {
                                            let _ = switch_view.update(cx, |view, cx| {
                                                view.compact_mode = checked;
                                                cx.notify();
                                            });
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(theme.metrics.spacing_3))
                                    .child(components::primary_button(
                                        theme,
                                        "foundation-action",
                                        "Test primitive",
                                        move |_, _, cx| {
                                            let _ = button_view.update(cx, |view, cx| {
                                                view.activation_count += 1;
                                                cx.notify();
                                            });
                                        },
                                    ))
                                    .child(
                                        div()
                                            .text_size(px(theme.typography.caption_size))
                                            .text_color(theme.colors.status.success)
                                            .child(status),
                                    ),
                            ),
                    ),
            )
    }
}

pub fn run() {
    gpui_platform::application().run(|cx: &mut App| {
        base_gpui::init(cx);

        let bounds = Bounds::centered(None, size(px(1040.0), px(720.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Probe".into()),
                    ..Default::default()
                }),
                window_min_size: Some(size(px(720.0), px(520.0))),
                app_id: Some("dev.probe.desktop".to_owned()),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| ProbeApp::new(window, cx)),
        )
        .expect("failed to open Probe's application window");

        cx.activate(true);
    });
}
