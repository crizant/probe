# Desktop Design System

## Purpose

Probe should feel at home on each supported operating system while retaining a clear,
consistent product identity. On macOS, the application follows the current Apple
Human Interface Guidelines and uses familiar macOS interaction patterns. On Windows,
it follows Microsoft Windows App Design and Fluent Design conventions. On Linux, it
uses common cross-desktop conventions informed by both GNOME and KDE guidance rather
than mechanically copying either toolkit.

This document defines the presentation contract for the GPUI desktop application.
Business behavior remains in the shared application and domain layers.

Official platform references:

- [Apple Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines/)
- [Microsoft Windows App Design](https://learn.microsoft.com/windows/apps/design/)
- [GNOME Human Interface Guidelines](https://developer.gnome.org/hig/)
- [KDE Human Interface Guidelines](https://develop.kde.org/hig/)

## Design Principles

- Prefer platform conventions over novelty.
- Keep the interface quiet, legible, and information-dense without feeling cramped.
- Make keyboard and pointer workflows equally complete.
- Preserve responsiveness and clarity with very large collections and responses.
- Use semantic design tokens so appearance can change without changing behavior.
- Treat accessibility and system appearance as foundational behavior.

## Native macOS Behavior

Before implementing an unfamiliar macOS pattern, consult the current official Apple
Human Interface Guidelines. In particular, verify:

- window, title-bar, toolbar, sidebar, split-view, sheet, and popover behavior;
- application menus, command placement, standard terminology, and keyboard shortcuts;
- first-responder behavior, focus movement, selection, and full keyboard access;
- system light/dark appearance, accent color, reduced motion, and increased contrast;
- accessibility labels, roles, states, hit targets, and assistive-technology behavior;
- destructive-action confirmation and restoration of reversible user state.

Use system behavior or a faithful GPUI equivalent where possible. Do not imitate a
native control visually while giving it surprising keyboard, focus, or accessibility
behavior.

## Native Windows Behavior

Before implementing an unfamiliar Windows pattern, consult the current Microsoft
Windows App Design guidance and Fluent Design conventions. In particular, verify:

- window frames, title bars, resizing, snapping, dialogs, flyouts, and context menus;
- command placement, standard terminology, access keys, and keyboard shortcuts;
- pointer, keyboard, touch, pen, and assistive input where applicable;
- system light/dark appearance, accent color, reduced motion, and high-contrast mode;
- display scaling, text scaling, accessibility roles, and screen-reader behavior;
- taskbar, notifications, file associations, and other shell integration when used.

GPUI does not make Probe a WinUI application, but Probe should reproduce the relevant
observable behavior and conventions faithfully. Do not add Fluent visual effects when
the corresponding behavior, accessibility, performance, or fallback cannot be
supported reliably.

## Cross-Desktop Linux Behavior

Linux does not have one universal HIG or native widget appearance. Probe therefore
uses a cross-desktop baseline:

- follow applicable freedesktop.org standards for desktop integration;
- use the GNOME HIG as a reference for GNOME conventions and accessibility;
- use the KDE HIG as a reference for Plasma conventions and configurable workflows;
- respect system fonts, light/dark preference, scaling, locale, input, and
  accessibility settings where the platform exposes them reliably;
- use familiar Linux keyboard, window, file-dialog, clipboard, notification, and
  drag-and-drop behavior;
- verify important workflows on both GNOME and KDE Plasma where practical.

Probe must not switch between a GTK/Adwaita imitation and a Qt/Breeze imitation based
on the detected desktop. It retains one coherent visual identity and adapts integration
and interaction where there is a meaningful desktop convention. When GNOME and KDE
guidance differs, prefer the least surprising cross-desktop behavior and document any
intentional desktop-specific adaptation.

## Platform Adaptation

Shared features use the same application operations and semantic intent on every
platform. Desktop adapters may vary menus, shortcuts, window chrome, control metrics,
and interaction details when platform conventions differ.

Platform-specific presentation must not leak into the domain, OpenCollection adapter,
HTTP engine, or CLI. A platform-specific choice must have an explicit fallback or
mapping for the other supported platforms before it becomes part of a shared component
API.

## Component Architecture

Longbridge `gpui-base` provides reusable behavior, accessibility, and default
chrome tokens where an appropriate primitive exists. Probe maps Catppuccin
Latte (light) and Mocha (dark) onto the semantic theme model and composes
feature views. Do not use Longbridge `gpui-component`.

```text
Longbridge gpui-base primitives
        ↓
Probe semantic tokens (Catppuccin Latte / Mocha)
        ↓
Probe styled component
        ↓
feature view
```

Feature views must not parse theme files, own global appearance state, or duplicate
interaction logic supplied by a shared component.

## Semantic Tokens

Tokens describe purpose, not literal appearance. The initial token model should cover
at least:

- surfaces: window, sidebar, editor, raised, overlay;
- text: primary, secondary, muted, placeholder, inverse;
- borders and separators: subtle, standard, strong, focused;
- actions: accent, hover, pressed, disabled;
- selection: active and inactive backgrounds and foregrounds;
- status: success, warning, error, and informational;
- request methods and response-status families;
- syntax and response-viewer roles;
- typography: interface, monospace, size, weight, and line height;
- spacing, radii, control sizes, icon sizes, and elevation where applicable;
- motion durations and easing, including a reduced-motion mapping.

Components consume semantic tokens rather than raw color literals or configuration
keys. Tokens that depend on interaction state must define normal, hovered, pressed,
focused, selected, inactive, and disabled behavior where relevant.

## Built-In Themes

The desktop foundation ships with Catppuccin [Latte](https://github.com/catppuccin/catppuccin)
(light) and [Mocha](https://github.com/catppuccin/catppuccin) (dark) as complete
semantic theme models, and follows system appearance changes. Built-in themes
provide fallback values for every token. HTTP method, status, and syntax colors
use Catppuccin accent hues and remain distinct from chrome.

Theme changes must not alter application semantics, hide required state, move commands
unexpectedly, or replace platform-standard interaction behavior. Selection, focus,
errors, disabled controls, and request/response status must remain distinguishable
without relying on color alone.

## Future Plain-Text Themes

User-defined themes will be stored in versioned, human-editable plain-text files. The
exact syntax is intentionally deferred until the semantic schema is stable.

The eventual theme subsystem must:

- parse and validate files outside components;
- produce the same semantic theme model used by built-in themes;
- preserve deterministic behavior across runs;
- report the source file and invalid field without crashing the desktop application;
- reject invalid types and unsupported schema versions;
- merge missing optional values with documented built-in fallbacks;
- retain a complete built-in theme when a custom theme cannot load;
- apply valid changes without rebuilding domain or workspace state;
- document every public token and compatibility rule;
- validate contrast and non-color state differentiation.

Theme files are local presentation configuration. They must not be stored in
OpenCollection YAML or treated as collection/domain data. Theme loading is not part of
the initial GPUI foundation phase.

## Interaction and Accessibility Review

Every new reusable desktop component should be reviewed in light and dark appearance
for:

- pointer, keyboard, and focus behavior;
- active and inactive window states;
- empty, loading, success, warning, error, and disabled states;
- text scaling, display scaling, truncation, and localization pressure;
- sufficient contrast and state cues beyond color;
- screen-reader naming, role, value, and state where supported;
- reduced-motion and increased-contrast settings where relevant.

Review appearance visually in light and dark. Do not add automated tests for UI
spacing, padding, radii, typography sizes, palette values, or contrast ratios.
Automated tests should cover behavior, domain, and integration — not visual design
tokens.
