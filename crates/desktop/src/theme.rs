//! Semantic desktop design tokens and complete built-in themes.
//!
//! Components consume these roles rather than embedding color literals. Future
//! user-authored themes can produce this same model without changing components.

use gpui::{Rgba, WindowAppearance, rgba};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeAppearance {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub appearance: ThemeAppearance,
    pub colors: Colors,
    pub typography: Typography,
    pub metrics: Metrics,
    pub motion: Motion,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    pub surfaces: SurfaceColors,
    pub text: TextColors,
    pub borders: BorderColors,
    pub actions: ActionColors,
    pub selection: SelectionColors,
    pub status: StatusColors,
    pub methods: MethodColors,
    pub responses: ResponseColors,
    pub syntax: SyntaxColors,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceColors {
    pub window: Rgba,
    pub sidebar: Rgba,
    pub editor: Rgba,
    pub raised: Rgba,
    pub overlay: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextColors {
    pub primary: Rgba,
    pub secondary: Rgba,
    pub muted: Rgba,
    pub placeholder: Rgba,
    pub inverse: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderColors {
    pub subtle: Rgba,
    pub standard: Rgba,
    pub strong: Rgba,
    pub focused: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActionColors {
    pub accent: Rgba,
    pub hover: Rgba,
    pub pressed: Rgba,
    pub disabled: Rgba,
    pub disabled_foreground: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionColors {
    pub active_background: Rgba,
    pub active_foreground: Rgba,
    pub inactive_background: Rgba,
    pub inactive_foreground: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StatusColors {
    pub success: Rgba,
    pub warning: Rgba,
    pub error: Rgba,
    pub informational: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MethodColors {
    pub get: Rgba,
    pub post: Rgba,
    pub put: Rgba,
    pub patch: Rgba,
    pub delete: Rgba,
    pub other: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResponseColors {
    pub informational: Rgba,
    pub success: Rgba,
    pub redirect: Rgba,
    pub client_error: Rgba,
    pub server_error: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyntaxColors {
    pub plain: Rgba,
    pub property: Rgba,
    pub string: Rgba,
    pub number: Rgba,
    pub boolean: Rgba,
    pub null: Rgba,
    pub punctuation: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Typography {
    pub interface_family: &'static str,
    pub monospace_family: &'static str,
    pub body_size: f32,
    pub caption_size: f32,
    pub title_size: f32,
    pub body_line_height: f32,
    pub regular_weight: u16,
    pub medium_weight: u16,
    pub semibold_weight: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub spacing_1: f32,
    pub spacing_2: f32,
    pub spacing_3: f32,
    pub spacing_4: f32,
    pub radius_small: f32,
    pub radius_medium: f32,
    pub radius_large: f32,
    pub control_height: f32,
    pub icon_small: f32,
    pub icon_standard: f32,
    pub elevation_raised: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub fast_ms: u16,
    pub standard_ms: u16,
    pub slow_ms: u16,
    pub easing: &'static str,
    pub reduced_duration_ms: u16,
}

impl Theme {
    #[must_use]
    pub fn for_window_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
        }
    }

    #[must_use]
    pub fn light() -> Self {
        Self {
            appearance: ThemeAppearance::Light,
            colors: Colors {
                surfaces: SurfaceColors {
                    window: rgba(0xf5f5f7ff),
                    sidebar: rgba(0xebebefff),
                    editor: rgba(0xffffffff),
                    raised: rgba(0xffffffff),
                    overlay: rgba(0xfffffff2),
                },
                text: TextColors {
                    primary: rgba(0x1d1d1fff),
                    secondary: rgba(0x4b4b50ff),
                    muted: rgba(0x6e6e73ff),
                    placeholder: rgba(0x8e8e93ff),
                    inverse: rgba(0xffffffff),
                },
                borders: BorderColors {
                    subtle: rgba(0x00000014),
                    standard: rgba(0x00000029),
                    strong: rgba(0x00000052),
                    focused: rgba(0x0066ccff),
                },
                actions: ActionColors {
                    accent: rgba(0x0066ccff),
                    hover: rgba(0x005bb8ff),
                    pressed: rgba(0x004c99ff),
                    disabled: rgba(0xd1d1d6ff),
                    disabled_foreground: rgba(0x76767bff),
                },
                selection: SelectionColors {
                    active_background: rgba(0x0066ccff),
                    active_foreground: rgba(0xffffffff),
                    inactive_background: rgba(0xd8d8dcff),
                    inactive_foreground: rgba(0x1d1d1fff),
                },
                status: StatusColors {
                    success: rgba(0x248a3dff),
                    warning: rgba(0x9a6700ff),
                    error: rgba(0xc62828ff),
                    informational: rgba(0x0066ccff),
                },
                methods: MethodColors {
                    get: rgba(0x087f5bff),
                    post: rgba(0x7a5af8ff),
                    put: rgba(0x9a6700ff),
                    patch: rgba(0xb54708ff),
                    delete: rgba(0xc62828ff),
                    other: rgba(0x4b4b50ff),
                },
                responses: ResponseColors {
                    informational: rgba(0x0066ccff),
                    success: rgba(0x248a3dff),
                    redirect: rgba(0x7a5af8ff),
                    client_error: rgba(0xb54708ff),
                    server_error: rgba(0xc62828ff),
                },
                syntax: SyntaxColors {
                    plain: rgba(0x1d1d1fff),
                    property: rgba(0x005cc5ff),
                    string: rgba(0x248a3dff),
                    number: rgba(0x7a3e9dff),
                    boolean: rgba(0xb54708ff),
                    null: rgba(0x6e6e73ff),
                    punctuation: rgba(0x4b4b50ff),
                },
            },
            typography: platform_typography(),
            metrics: default_metrics(),
            motion: default_motion(),
        }
    }

    #[must_use]
    pub fn dark() -> Self {
        Self {
            appearance: ThemeAppearance::Dark,
            colors: Colors {
                surfaces: SurfaceColors {
                    window: rgba(0x1c1c1eff),
                    sidebar: rgba(0x242426ff),
                    editor: rgba(0x161618ff),
                    raised: rgba(0x2c2c2eff),
                    overlay: rgba(0x303033f2),
                },
                text: TextColors {
                    primary: rgba(0xf5f5f7ff),
                    secondary: rgba(0xd1d1d6ff),
                    muted: rgba(0xaeaeb2ff),
                    placeholder: rgba(0x8e8e93ff),
                    inverse: rgba(0x1d1d1fff),
                },
                borders: BorderColors {
                    subtle: rgba(0xffffff1f),
                    standard: rgba(0xffffff38),
                    strong: rgba(0xffffff66),
                    focused: rgba(0x66aaffff),
                },
                actions: ActionColors {
                    accent: rgba(0x0a84ffff),
                    hover: rgba(0x3599ffff),
                    pressed: rgba(0x0071e3ff),
                    disabled: rgba(0x48484aff),
                    disabled_foreground: rgba(0xaaaab0ff),
                },
                selection: SelectionColors {
                    active_background: rgba(0x0066ccff),
                    active_foreground: rgba(0xffffffff),
                    inactive_background: rgba(0x48484aff),
                    inactive_foreground: rgba(0xf5f5f7ff),
                },
                status: StatusColors {
                    success: rgba(0x4cc963ff),
                    warning: rgba(0xffd60aff),
                    error: rgba(0xff6961ff),
                    informational: rgba(0x64d2ffff),
                },
                methods: MethodColors {
                    get: rgba(0x63e6beff),
                    post: rgba(0xbf9affff),
                    put: rgba(0xffd60aff),
                    patch: rgba(0xff9f0aff),
                    delete: rgba(0xff6961ff),
                    other: rgba(0xd1d1d6ff),
                },
                responses: ResponseColors {
                    informational: rgba(0x64d2ffff),
                    success: rgba(0x4cc963ff),
                    redirect: rgba(0xbf9affff),
                    client_error: rgba(0xff9f0aff),
                    server_error: rgba(0xff6961ff),
                },
                syntax: SyntaxColors {
                    plain: rgba(0xf5f5f7ff),
                    property: rgba(0x64d2ffff),
                    string: rgba(0x63e6beff),
                    number: rgba(0xbf9affff),
                    boolean: rgba(0xff9f0aff),
                    null: rgba(0xaeaeb2ff),
                    punctuation: rgba(0xd1d1d6ff),
                },
            },
            typography: platform_typography(),
            metrics: default_metrics(),
            motion: default_motion(),
        }
    }
}

const fn platform_typography() -> Typography {
    Typography {
        interface_family: interface_font(),
        monospace_family: monospace_font(),
        body_size: 13.0,
        caption_size: 11.0,
        title_size: 24.0,
        body_line_height: 1.45,
        regular_weight: 400,
        medium_weight: 500,
        semibold_weight: 600,
    }
}

#[cfg(target_os = "macos")]
const fn interface_font() -> &'static str {
    ".SystemUIFont"
}

#[cfg(target_os = "windows")]
const fn interface_font() -> &'static str {
    "Segoe UI Variable"
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const fn interface_font() -> &'static str {
    "sans-serif"
}

#[cfg(target_os = "macos")]
const fn monospace_font() -> &'static str {
    "SF Mono"
}

#[cfg(target_os = "windows")]
const fn monospace_font() -> &'static str {
    "Cascadia Mono"
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const fn monospace_font() -> &'static str {
    "monospace"
}

const fn default_metrics() -> Metrics {
    Metrics {
        spacing_1: 4.0,
        spacing_2: 8.0,
        spacing_3: 12.0,
        spacing_4: 16.0,
        radius_small: 5.0,
        radius_medium: 8.0,
        radius_large: 12.0,
        control_height: 30.0,
        icon_small: 12.0,
        icon_standard: 16.0,
        elevation_raised: 1.0,
    }
}

const fn default_motion() -> Motion {
    Motion {
        fast_ms: 80,
        standard_ms: 160,
        slow_ms: 240,
        easing: "cubic-bezier(0.2, 0, 0, 1)",
        reduced_duration_ms: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{Theme, ThemeAppearance};
    use gpui::{Rgba, WindowAppearance};

    #[test]
    fn native_appearances_select_the_expected_built_in_theme() {
        assert_eq!(
            Theme::for_window_appearance(WindowAppearance::Light).appearance,
            ThemeAppearance::Light
        );
        assert_eq!(
            Theme::for_window_appearance(WindowAppearance::VibrantLight).appearance,
            ThemeAppearance::Light
        );
        assert_eq!(
            Theme::for_window_appearance(WindowAppearance::Dark).appearance,
            ThemeAppearance::Dark
        );
        assert_eq!(
            Theme::for_window_appearance(WindowAppearance::VibrantDark).appearance,
            ThemeAppearance::Dark
        );
    }

    #[test]
    fn built_in_themes_define_distinct_state_tokens() {
        for theme in [Theme::light(), Theme::dark()] {
            assert_ne!(theme.colors.actions.accent, theme.colors.actions.hover);
            assert_ne!(theme.colors.actions.hover, theme.colors.actions.pressed);
            assert_ne!(theme.colors.borders.subtle, theme.colors.borders.focused);
            assert_ne!(
                theme.colors.selection.active_background,
                theme.colors.selection.inactive_background
            );
            assert!(theme.metrics.control_height >= 28.0);
        }
    }

    #[test]
    fn built_in_themes_keep_primary_content_and_controls_legible() {
        for theme in [Theme::light(), Theme::dark()] {
            let editor = theme.colors.surfaces.editor;
            assert!(contrast_ratio(theme.colors.text.primary, editor) >= 7.0);
            assert!(contrast_ratio(theme.colors.text.secondary, editor) >= 4.5);
            assert!(contrast_ratio(theme.colors.text.inverse, theme.colors.actions.accent) >= 4.5);
            assert!(
                contrast_ratio(
                    theme.colors.selection.active_foreground,
                    theme.colors.selection.active_background
                ) >= 4.5
            );
        }
    }

    fn contrast_ratio(a: Rgba, b: Rgba) -> f32 {
        let light = luminance(a).max(luminance(b));
        let dark = luminance(a).min(luminance(b));
        (light + 0.05) / (dark + 0.05)
    }

    fn luminance(color: Rgba) -> f32 {
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }

    fn linear(channel: f32) -> f32 {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    }
}
