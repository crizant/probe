//! Semantic desktop design tokens and complete built-in themes.
//!
//! Built-in appearances map [Catppuccin](https://github.com/catppuccin/catppuccin)
//! Latte (light) and Mocha (dark) onto this model. Components consume roles rather
//! than embedding color literals. Future user-authored themes can produce the same
//! model without changing components.

use std::borrow::Cow;

use gpui::{FontWeight, Hsla, Rgba, WindowAppearance, hsla, px, rgba};
use gpui_base::{
    ColorTokens, RadiusTokens, ScrollbarStyles, SemanticThemeTokens, ShadowTokens, SpacingTokens,
    TextStyleToken, TypographyTokens,
};

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
    pub tree_row_height: f32,
    pub tab_bar_height: f32,
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

    /// Install gpui-base infrastructure and the library's default semantic tokens.
    pub fn init(cx: &mut gpui::App) {
        gpui_base::init(cx);
        load_bundled_fonts(cx);
        Self::sync_gpui_base(WindowAppearance::Light, cx);
    }

    /// Project this theme into gpui-base's global `Theme` so primitives and
    /// infrastructure (scrollbars, resize handles) share the same tokens.
    pub fn sync_gpui_base(appearance: WindowAppearance, cx: &mut gpui::App) {
        let theme = Self::for_window_appearance(appearance);
        let gpui_theme = gpui_base::Theme::global_mut(cx);
        gpui_theme.tokens = theme.semantic_tokens();
        let muted: Hsla = theme.colors.text.muted.into();
        let border: Hsla = theme.colors.borders.standard.into();
        let primary: Hsla = theme.colors.text.primary.into();
        gpui_theme.scrollbar.styles = ScrollbarStyles::default()
            .thumb(|thumb| thumb.bg(muted))
            .thumb_hover(|thumb| thumb.bg(border))
            .thumb_active(|thumb| thumb.bg(primary));
        gpui_theme.resizable.handle = theme.colors.borders.subtle.into();
        gpui_theme.resizable.active_handle = theme.colors.borders.focused.into();
    }

    /// Editor caret selection wash derived from Catppuccin blue.
    #[must_use]
    pub fn editor_selection(self) -> Hsla {
        match self.appearance {
            ThemeAppearance::Light => hsla(0.611, 0.915, 0.539, 0.28),
            ThemeAppearance::Dark => hsla(0.603, 0.919, 0.759, 0.30),
        }
    }

    fn semantic_tokens(self) -> SemanticThemeTokens {
        let mut shadow: Hsla = self.colors.text.primary.into();
        shadow.a = match self.appearance {
            ThemeAppearance::Light => 0.10,
            ThemeAppearance::Dark => 0.32,
        };
        let body_line = px(self.typography.body_size * self.typography.body_line_height);
        SemanticThemeTokens {
            colors: ColorTokens {
                background: self.colors.surfaces.window.into(),
                foreground: self.colors.text.primary.into(),
                surface: self.colors.surfaces.raised.into(),
                surface_foreground: self.colors.text.primary.into(),
                primary: self.colors.actions.accent.into(),
                primary_foreground: self.colors.text.inverse.into(),
                secondary: self.colors.surfaces.raised.into(),
                secondary_foreground: self.colors.text.secondary.into(),
                muted: self.colors.surfaces.sidebar.into(),
                muted_foreground: self.colors.text.muted.into(),
                accent: self.colors.selection.inactive_background.into(),
                accent_foreground: self.colors.text.primary.into(),
                destructive: self.colors.status.error.into(),
                destructive_foreground: self.colors.text.inverse.into(),
                border: self.colors.borders.standard.into(),
                input: self.colors.borders.standard.into(),
                ring: self.colors.borders.focused.into(),
            },
            radius: RadiusTokens {
                none: px(0.0),
                sm: px(self.metrics.radius_small),
                md: px(self.metrics.radius_medium),
                lg: px(self.metrics.radius_large),
                xl: px(self.metrics.radius_large),
                full: px(9999.0),
            },
            spacing: SpacingTokens {
                xxs: px(2.0),
                xs: px(self.metrics.spacing_1),
                sm: px(self.metrics.spacing_2),
                md: px(self.metrics.spacing_3),
                lg: px(self.metrics.spacing_4),
                xl: px(20.0),
                xxl: px(24.0),
            },
            typography: TypographyTokens {
                sans: self.typography.interface_family.into(),
                mono: self.typography.monospace_family.into(),
                xs: TextStyleToken {
                    size: px(self.typography.caption_size),
                    line_height: px(16.0),
                    weight: FontWeight::NORMAL,
                },
                sm: TextStyleToken {
                    size: px(self.typography.body_size),
                    line_height: body_line,
                    weight: FontWeight::NORMAL,
                },
                md: TextStyleToken {
                    size: px(self.typography.body_size),
                    line_height: body_line,
                    weight: FontWeight::NORMAL,
                },
                lg: TextStyleToken {
                    size: px(15.0),
                    line_height: px(22.0),
                    weight: FontWeight::NORMAL,
                },
                xl: TextStyleToken {
                    size: px(self.typography.title_size),
                    line_height: px(32.0),
                    weight: FontWeight::SEMIBOLD,
                },
                mono_md: TextStyleToken {
                    size: px(self.typography.body_size),
                    line_height: px(20.0),
                    weight: FontWeight::NORMAL,
                },
            },
            shadow: ShadowTokens::elevations(shadow),
        }
    }

    #[must_use]
    pub fn method_color(self, method: &str) -> Rgba {
        match method.to_ascii_uppercase().as_str() {
            "GET" => self.colors.methods.get,
            "POST" => self.colors.methods.post,
            "PUT" => self.colors.methods.put,
            "PATCH" => self.colors.methods.patch,
            "DELETE" => self.colors.methods.delete,
            _ => self.colors.methods.other,
        }
    }

    #[must_use]
    pub fn light() -> Self {
        Self {
            appearance: ThemeAppearance::Light,
            colors: latte(),
            typography: platform_typography(),
            metrics: default_metrics(),
            motion: default_motion(),
        }
    }

    #[must_use]
    pub fn dark() -> Self {
        Self {
            appearance: ThemeAppearance::Dark,
            colors: mocha(),
            typography: platform_typography(),
            metrics: default_metrics(),
            motion: default_motion(),
        }
    }
}

/// Catppuccin Latte — official palette mapped onto Probe semantic roles.
/// https://github.com/catppuccin/catppuccin
fn latte() -> Colors {
    Colors {
        surfaces: SurfaceColors {
            window: rgba(0xeff1f5ff),  // base
            sidebar: rgba(0xe6e9efff), // mantle
            editor: rgba(0xeff1f5ff),  // base
            raised: rgba(0xdce0e8ff),  // crust
            overlay: rgba(0xeff1f5ff), // base
        },
        text: TextColors {
            primary: rgba(0x4c4f69ff),     // text
            secondary: rgba(0x5c5f77ff),   // subtext1
            muted: rgba(0x6c6f85ff),       // subtext0
            placeholder: rgba(0x8c8fa1ff), // overlay1
            inverse: rgba(0xffffffff),     // on-accent
        },
        borders: BorderColors {
            subtle: rgba(0xccd0daff),   // surface0
            standard: rgba(0xbcc0ccff), // surface1
            strong: rgba(0x9ca0b0ff),   // overlay0
            focused: rgba(0x1e66f5ff),  // blue
        },
        actions: ActionColors {
            accent: rgba(0x1e66f5ff),              // blue
            hover: rgba(0x0a52e0ff),               // blue, darker
            pressed: rgba(0x0843b9ff),             // blue, darkest
            disabled: rgba(0xccd0daff),            // surface0
            disabled_foreground: rgba(0x8c8fa1ff), // overlay1
        },
        selection: SelectionColors {
            active_background: rgba(0x1e66f5ff),   // blue
            active_foreground: rgba(0xffffffff),   // on-accent
            inactive_background: rgba(0xccd0daff), // surface0
            inactive_foreground: rgba(0x4c4f69ff), // text
        },
        status: StatusColors {
            success: rgba(0x40a02bff),       // green
            warning: rgba(0xdf8e1dff),       // yellow
            error: rgba(0xd20f39ff),         // red
            informational: rgba(0x209fb5ff), // sapphire
        },
        methods: MethodColors {
            get: rgba(0x179299ff),    // teal
            post: rgba(0xfe640bff),   // peach
            put: rgba(0xdf8e1dff),    // yellow
            patch: rgba(0x8839efff),  // mauve
            delete: rgba(0xd20f39ff), // red
            other: rgba(0x7c7f93ff),  // overlay2
        },
        responses: ResponseColors {
            informational: rgba(0x209fb5ff), // sapphire
            success: rgba(0x40a02bff),       // green
            redirect: rgba(0xdf8e1dff),      // yellow
            client_error: rgba(0xfe640bff),  // peach
            server_error: rgba(0xd20f39ff),  // red
        },
        syntax: SyntaxColors {
            plain: rgba(0x4c4f69ff),       // text
            property: rgba(0x1e66f5ff),    // blue
            string: rgba(0x40a02bff),      // green
            number: rgba(0xfe640bff),      // peach
            boolean: rgba(0x8839efff),     // mauve
            null: rgba(0x7c7f93ff),        // overlay2
            punctuation: rgba(0x8c8fa1ff), // overlay1
        },
    }
}

/// Catppuccin Mocha — official palette mapped onto Probe semantic roles.
/// https://github.com/catppuccin/catppuccin
fn mocha() -> Colors {
    Colors {
        surfaces: SurfaceColors {
            window: rgba(0x1e1e2eff),  // base
            sidebar: rgba(0x181825ff), // mantle
            editor: rgba(0x1e1e2eff),  // base
            raised: rgba(0x11111bff),  // crust
            overlay: rgba(0x313244ff), // surface0
        },
        text: TextColors {
            primary: rgba(0xcdd6f4ff),     // text
            secondary: rgba(0xbac2deff),   // subtext1
            muted: rgba(0xa6adc8ff),       // subtext0
            placeholder: rgba(0x7f849cff), // overlay1
            inverse: rgba(0x11111bff),     // crust
        },
        borders: BorderColors {
            subtle: rgba(0x313244ff),   // surface0
            standard: rgba(0x45475aff), // surface1
            strong: rgba(0x6c7086ff),   // overlay0
            focused: rgba(0x89b4faff),  // blue
        },
        actions: ActionColors {
            accent: rgba(0x89b4faff),              // blue
            hover: rgba(0xa1c4fbff),               // blue, lighter
            pressed: rgba(0xbad3fcff),             // blue, lightest
            disabled: rgba(0x313244ff),            // surface0
            disabled_foreground: rgba(0x7f849cff), // overlay1
        },
        selection: SelectionColors {
            active_background: rgba(0x89b4faff),   // blue
            active_foreground: rgba(0x11111bff),   // crust
            inactive_background: rgba(0x313244ff), // surface0
            inactive_foreground: rgba(0xcdd6f4ff), // text
        },
        status: StatusColors {
            success: rgba(0xa6e3a1ff),       // green
            warning: rgba(0xf9e2afff),       // yellow
            error: rgba(0xf38ba8ff),         // red
            informational: rgba(0x74c7ecff), // sapphire
        },
        methods: MethodColors {
            get: rgba(0x94e2d5ff),    // teal
            post: rgba(0xfab387ff),   // peach
            put: rgba(0xf9e2afff),    // yellow
            patch: rgba(0xcba6f7ff),  // mauve
            delete: rgba(0xf38ba8ff), // red
            other: rgba(0x9399b2ff),  // overlay2
        },
        responses: ResponseColors {
            informational: rgba(0x74c7ecff), // sapphire
            success: rgba(0xa6e3a1ff),       // green
            redirect: rgba(0xf9e2afff),      // yellow
            client_error: rgba(0xfab387ff),  // peach
            server_error: rgba(0xf38ba8ff),  // red
        },
        syntax: SyntaxColors {
            plain: rgba(0xcdd6f4ff),       // text
            property: rgba(0x89b4faff),    // blue
            string: rgba(0xa6e3a1ff),      // green
            number: rgba(0xfab387ff),      // peach
            boolean: rgba(0xcba6f7ff),     // mauve
            null: rgba(0x9399b2ff),        // overlay2
            punctuation: rgba(0x7f849cff), // overlay1
        },
    }
}

const fn platform_typography() -> Typography {
    Typography {
        interface_family: interface_font(),
        monospace_family: monospace_font(),
        body_size: 13.0,
        caption_size: 12.0,
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

const fn monospace_font() -> &'static str {
    "JetBrains Mono"
}

/// JetBrains Mono 2.304 (SIL OFL 1.1), bundled in `assets/fonts/jetbrains-mono`.
fn load_bundled_fonts(cx: &mut gpui::App) {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(
                include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf")
                    .as_slice(),
            ),
            Cow::Borrowed(
                include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-Medium.ttf")
                    .as_slice(),
            ),
            Cow::Borrowed(
                include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-SemiBold.ttf")
                    .as_slice(),
            ),
        ])
        .expect("failed to load bundled JetBrains Mono fonts");
}

const fn default_metrics() -> Metrics {
    Metrics {
        spacing_1: 4.0,
        spacing_2: 8.0,
        spacing_3: 12.0,
        spacing_4: 16.0,
        radius_small: 6.0,
        radius_medium: 8.0,
        radius_large: 10.0,
        control_height: 28.0,
        tree_row_height: 28.0,
        tab_bar_height: 32.0,
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
    use gpui::{Rgba, WindowAppearance, rgba};

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
            assert!(theme.typography.caption_size >= 12.0);
            assert!(theme.typography.body_size >= theme.typography.caption_size);
            assert_eq!(theme.typography.monospace_family, "JetBrains Mono");
            assert!(theme.metrics.radius_small >= 4.0);
            assert!(theme.metrics.radius_large <= 12.0);
            assert!(theme.metrics.radius_small < theme.metrics.radius_medium);
            assert!(theme.metrics.radius_medium < theme.metrics.radius_large);
            assert_eq!(theme.method_color("GET"), theme.colors.methods.get);
            assert_eq!(theme.method_color("post"), theme.colors.methods.post);
            assert_ne!(theme.method_color("GET"), theme.method_color("DELETE"));
        }
    }

    #[test]
    fn built_in_themes_use_catppuccin_latte_and_mocha() {
        assert_eq!(Theme::light().colors.surfaces.window, rgba(0xeff1f5ff));
        assert_eq!(Theme::light().colors.actions.accent, rgba(0x1e66f5ff));
        assert_eq!(Theme::dark().colors.surfaces.window, rgba(0x1e1e2eff));
        assert_eq!(Theme::dark().colors.actions.accent, rgba(0x89b4faff));
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
