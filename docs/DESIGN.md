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
chrome tokens where an appropriate primitive exists. Probe maps Porcelain Honey
(light) and Graphite Honey (dark) onto the semantic theme model and composes
feature views. Do not use Longbridge `gpui-component`.

```text
Longbridge gpui-base primitives
        ↓
Probe semantic tokens (Porcelain Honey / Graphite Honey)
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

The desktop foundation ships with Porcelain Honey (light), based on Probe's
creamy app icon background, and Graphite Honey (dark), a graphite companion theme
with the same golden-orange accent system, as complete semantic theme models.
With the user configuration `theme` set to `system` (the default), built-in
themes follow OS appearance changes. `light` keeps Porcelain Honey and `dark`
keeps Graphite Honey when the OS appearance changes. On macOS, `light` and
`dark` also set the native window appearance so the titlebar matches; `system`
leaves that appearance to the OS. Every token has a fallback value. HTTP
method, status, and syntax colors remain distinct from chrome. See
[user configuration](ARCHITECTURE.md#user-configuration).

Settings opens from Probe → Settings on macOS or File → Settings on Windows/Linux
(Command+, / Ctrl+,). Its Theme mode row has the label on the left and a
System/Light/Dark dropdown on the right. Save (Command+Enter / Ctrl+Enter) applies
and persists changes; Cancel or Escape discards them. Save is disabled until a
change is made, and controls are disabled while saving. A failed save keeps the
draft open and the current theme unchanged. Enter and Escape retain normal
dropdown behavior while its list is open.

Theme changes must not alter application semantics, hide required state, move commands
unexpectedly, or replace platform-standard interaction behavior. Selection, focus,
errors, disabled controls, and request/response status must remain distinguishable
without relying on color alone.

## Variable Placeholders

Request fields highlight two kinds of placeholders. `{{name}}` is an environment
variable from the selected environment. `:name` in the URL path is a path parameter
on that request. Only the URL bar highlights path placeholders; other fields ignore
`:name`.

A placeholder is painted by whether it would be substituted if the request ran now.
Resolved placeholders use the syntax string color and no underline. Unresolved
placeholders use the status error color and a 1px underline, so the state stays
visible without color.

Unresolved covers an environment variable that is absent or disabled, a secret Probe
has learned is not stored, a path-parameter row that is missing, disabled, or still
blank, and every `{{name}}` when no environment is selected or environment resolution
failed. An environment variable set to an empty string is resolved. A secret is
resolved only when credential-presence metadata says it is stored, and unresolved
only when that metadata says it is known missing. A secret whose presence is unknown
uses the secondary text color, no error underline, and a lock in a tooltip that says
"Not verified". Presence metadata and its trust rules are defined in
[Architecture](ARCHITECTURE.md#presence-metadata). The stored-secret tooltip says
"Stored securely" and offers Replace Secret. Known-missing and unverified secrets
offer Set Secret. Secret tooltips never render a value input; both actions open the
secret value dialog. A blank path parameter is not resolved, because those rows are
created empty as soon as `:name` is typed.

`:name` is never classified against the environment, and `{{name}}` is never
classified against path parameters, even when the names coincide. Highlights and
hover tooltips share `probe_core::VariableStatus`. Path placeholders are edited in
the path-parameters section and do not use the environment tooltip.

## Secret Values

The Environment Manager lists secret declarations alongside plain variables. An
inherited declaration shows the environment that defines it. Each secret row shows
its presence as stored, not set (known missing), or not verified (unknown), and
offers Set or Replace accordingly. Adding a secret creates a draft declaration;
credential actions stay disabled until the environment draft is saved, because the
stored value belongs to the saved identity.

Set and Replace open the same secret value dialog from the Environment Manager or
from a placeholder tooltip. The dialog is masked, starts empty even when replacing,
clears its input when the value is submitted or the dialog closes, submits on Enter,
discards on Escape, and returns focus to the control that opened it. Only one
credential write runs at a time. Delete Stored Value is offered only while replacing
a stored value, requires confirmation, and keeps the declaration. Removing a
declaration from the Environment Manager does not delete its stored value.

Saving an environment rename, or a rename of a direct secret variable, asks for
confirmation when the change would leave stored values under the previous identity.
The prompt states that affected values must be stored again and names the variable
when a single secret was renamed. It is based on the declaration edit and does not
query the credential store. Credential store failures are reported without changing presence or the
collection.

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


### Assertion surfaces

HTTP and GraphQL Request Editors expose a scrollable Assertions list using the
shared key/value row builders, keyboard-accessible enable switches, variable-aware
expression/value inputs, and an operator menu. Title Case Add Assertion and Remove
Assertion controls preserve persisted order. Every assertion has a compact, indented
Description input on its second line, including when empty; editing preserves
structured descriptions and unused unary values. The expected-value field is empty
and disabled for `isNull` and `isNotNull`, showing “Not used”; switching back to a
comparison restores its previous value. Assertion outcomes appear only
in the response viewer.

Response tab counts use compact rounded rectangular badges that expand with their
content, without literal brackets. Inspect uses a neutral count badge. The Assertions
tab shows failure and evaluation-error counts with SVG status icons, subtle tinted
backgrounds, and semantic error and warning colours. Zero counts are hidden; tooltips
and the accessible tab label explain each count. Counts remain visible when another
response tab is selected. The tab content has one inline summary with independent,
nonzero counts: failed, evaluation errors, passed, and disabled, in that order.
Each count uses its own semantic colour: error, warning, success, or muted text.
The all-disabled message uses the same left-aligned summary layout and muted text.
Failed comparisons precede evaluation errors, passed checks, and disabled checks,
with stable order within each group. Rows show the original,
uninterpolated declaration text; a description becomes the primary label when
present. Rows share padding and a minimum height. Failed rows have a semantic error
border and subtle highlighting, with redacted actual values inline after the comparison.
Descriptions, declarations, values, and error reasons wrap to fit the pane. Errors
use warning icons and shared, value-free reasons. Status icons and written
disabled/error information accompany semantic colours. Disabled labels sit below
row comparisons so the expression and comparison columns retain their alignment.
The no-assertions state centers its explanatory text vertically and horizontally
without an Add Assertion button; users add assertions in the request editor.
Other empty states use shared placeholders; request failures report that assertions
were not evaluated. Muted icon buttons at the
right of each row open its editor declaration or copy its result, with tooltips
and keyboard focus. Rows have no selection state.
Status icons align with the first text line, including in wrapped rows.

Response access remains available after assertion failures. Both surfaces use
existing theme tokens and controls; text buttons and tabs retain an explicit
`line_height(relative(1.25))`.

### Import from cURL

The sidebar Add (+) menu offers **Import from cURL…** for an open collection.
Its compact dialog contains a request name, destination folder (initially the
selected folder or selected request's parent, otherwise collection root), and a
multiline POSIX cURL command. Folder labels include their unique collection
selectors to distinguish same-named folders. If a destination is removed during
a reload, Import stays disabled until the user explicitly selects a destination.
Parsing runs in the background through the shared parser; pasted commands are
never executed. A read-only preview shows method, URL, and header, parameter, and
body counts. Authentication headers, cookies, and Basic credentials trigger a
warning that imported values persist in plaintext.

Import Request stays disabled until the name and command are valid and workspace
writes/reloads have finished. Command+Enter / Ctrl+Enter imports; Enter in the
command inserts a newline. Escape or Cancel discards the dialog without writing.
Once Import starts, cancellation is disabled until the atomic write completes.
Validation and write errors preserve input and display an inline recovery message.
A committed write whose refresh failed disables retry and asks the user to reopen
the collection, preventing duplicate imports. Successful imports open and select
the new request while preserving existing request drafts and editor state.

The dialog uses the shared semantic dialog surfaces, field spacing, focus rings,
and action buttons in both light and dark themes. Text-bearing buttons retain
`line_height(relative(1.25))`.
