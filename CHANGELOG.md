# Changelog

User-facing changes in Probe, newest release first. This changelog follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Release dates use Australia/Brisbane time (UTC+10).

## [Unreleased]

### Added

- Import a cURL command from stdin or a file into an existing bundled or unbundled
  workspace with `probe request import curl`, including folder and insertion position
  selection, without executing the request. Imported literal secrets are stored in
  plaintext OpenCollection YAML.

- Import an HTTP request from a pasted cURL command through the desktop sidebar
  Add menu or File → Import From → cURL, with a parsed preview, validation, and
  plaintext credential warnings.

### Changed

- Save Request, New HTTP Request, New GraphQL Request, and New Folder share a
  destination folder tree styled like the collection sidebar. These dialogs and
  Add/Edit Certificate submit with Command+Enter on macOS or Ctrl+Enter on
  Windows/Linux, with the shortcut shown on the submit button.
- Removed the Move dialog; collection items can still be moved by dragging them
  in the sidebar.

## [2.1.1] - 2026-10-25

### Added

- Settings dialog for choosing System, Light, or Dark theme mode, applied and
  remembered when saved.

## [2.1.0] - 2026-10-23

### Added

- HTTP and GraphQL request tabs can temporarily select a matching client
  certificate or send without one from the URL-bar certificate popover. Choices
  stay in memory, preserve per-tab selection, and are revalidated before sending.
  The popover shows public metadata and expiry warnings without querying secrets.

- Environment Manager client certificates now show public subject and expiry
  metadata, with expiry warnings. The certificate editor can inspect current PEM
  and PKCS#12 fields, copy SHA-256 fingerprints, and validate local key matches
  without sending a request. Protected certificates unlock only when Inspect is
  clicked; inspection and expiry never prevent saving.

### Fixed

- The empty response Assertions view centers its explanatory message and leaves
  Add Assertion in the request editor.

- Assertion summaries show separate failed, evaluation-error, passed, and disabled
  counts, so assertions that could not be evaluated are not mistaken for passes.
  Each outcome uses its status colour. Disabled assertions are labelled “Disabled”
  below their comparison so columns stay aligned, and the all-disabled message
  aligns with other summaries.

- Response Assertions tabs now distinguish failed assertions and evaluation errors
  with compact rectangular status badges and explanatory tooltips. Inspect counts
  use the same badge style.

- Environment Manager uses aligned text buttons for Add Variable, Add Secret, and
  Add Certificate, with distinct list footers. Certificates show readable expiry
  dates with warning icons.

- Text buttons now keep their hover and clickable areas sized to their content
  instead of stretching across popup rows.

## [2.0.0] - 2026-10-22

### Added

- Desktop HTTP and GraphQL requests now have an Assertions editor with an enable
  switch, expression, operator, expected value, and description fields using the
  normal Save workflow. Every send evaluates its declarations and shows
  Passed, Failed, Evaluation Error, and Skipped results in a response Assertions
  tab, including for truncated responses. Failed checks keep the response visible.
  Plain assertions retain full response paging and streamed file saves; direct
  saves consistently refuse requests that may use secrets. Adding then deleting
  an unsaved assertion restores the original absent or explicitly empty list.

- CLI assertion checks run against original responses before secret redaction,
  using the request's environment, overrides, and cached secret provider. JSON,
  human, and quiet output keep resolved secrets out of assertion results.

### Changed

- CLI assertion failures now identify each failed or errored check by source and
  index, with expressions, operators, redacted expected and actual values, or safe
  error diagnostics. Successful human
  output separates the response body and assertion summary with a blank line.
  JSON and quiet output are unchanged.

- Desktop assertion results now group failed checks first and show an inline
  summary, a failure count in the tab, descriptions, safe actual values, and
  distinct skipped and evaluation-error states. Keyboard-accessible copy and
  jump-to-editor buttons on each result replace row selection. Description inputs are
  always visible, and result rows share compact padding with inline actual values
  and wrapping for long text. Unary expected-value fields are empty and disabled,
  with their previous values restored when switching back to comparisons. Assertion
  outcomes appear in the response viewer, keeping the request editor focused on
  definitions and enable switches.
- Shared assertion results and CLI JSON now include redacted actual display values
  and truncation metadata as additive JSON fields. Values are redacted before
  truncation; existing CLI JSON fields, quiet diagnostics, flags, and exit codes
  are unchanged.

### Breaking changes from Probe v1.1.1

- Live CLI HTTP/GraphQL runs now evaluate persisted OpenCollection assertions.
  Disabled checks do not affect success; `--no-assertions` skips collection checks.
  Failed or errored assertions exit 9 with `assertion_failed`.
- Removing `--expect` (including its status OR shorthand), renaming the JSON
  field `expectations` to `assertions`, and renaming the error category
  `expectation_failed` to `assertion_failed` are breaking changes from v1.1.1.
  The next release requires a major version bump under semver. Use repeatable
  `--assert 'response.status equals 200'` or body checks with the shared eight
  operators; all active checks must pass.

## [1.1.1] - 2026-10-20

### Fixed

- File tree hover and selection highlights are easier to distinguish in dark mode.

- Request tab titles and remaining text buttons keep descenders such as g, p,
  and y fully visible.

- Desktop responses that finish while the collection reloads from disk now
  appear instead of leaving the response viewer empty or stuck formatting,
  inspecting, encoding, or loading a page. Send and Save sends the reloaded
  request after its destination is chosen.

## [1.1.0] - 2026-10-20

### Added

- HTTP and GraphQL OpenCollection runtime assertions now load, validate, and
  persist through bundled and unbundled request saves and collection copies.
  Assertion declarations are preserved without evaluation.

- The desktop Environment Manager now has Variables, Secrets, and Client Certificates
  tabs. Add, edit, disable, reorder, and remove environment PEM and PKCS#12
  declarations, including variable paths and masked literal or templated passphrases.

- HTTP and GraphQL requests now execute single-identity PKCS#12 (`.p12`/`.pfx`)
  client certificates alongside PEM in CLI and desktop, with existing variable
  and secret support. Modern PBES2/PBKDF2 AES-CBC containers are supported;
  legacy PBE encryption and multi-identity selection remain unsupported.

- Client certificate domains, PEM paths, and passphrases now support Probe
  environment variables and `--var` overrides, including secret variables.
  Dry-run and Copy as cURL leave certificate-only secrets unresolved.

- OpenCollection PEM and PKCS#12 client certificate configuration now loads and
  persists at collection and environment scope, with strict entry validation and
  indexed errors. Passphrases must be strings when present; explicit null is
  rejected.
- HTTP and GraphQL requests now use matching OpenCollection PEM client certificates
  for mTLS from CLI and desktop. Certificate-bearing requests connect directly,
  bypassing proxies. Domains match exact hosts or whole leftmost wildcard labels
  such as `*.example.com`. Direct selected-environment certificates take
  precedence over collection certificates. RSA, EC, PKCS#8, certificate chains,
  and encrypted PKCS#8 keys are supported. Redirects outside the selected domain
  fail with a clear diagnostic before connecting. Environment certificate
  inheritance remains unsupported; no proprietary fields are added.

### Changed

- Desktop editor carets now use the fixed 1px width provided by upstream GPUI Kit.
- JSON editors now use native smart indentation on Enter, including GraphQL
  Variables and Extensions.
- JSON body editors auto-close braces, brackets, and double quotes using tracked
  pairs; single quotes, parentheses, and backticks no longer auto-close. Plain
  text and XML remain unpaired.

### Fixed

- Desktop Copy as cURL no longer starts the HTTP execution runtime. The copied
  command is unchanged.

## [1.0.1] - 2026-10-14

### Changed

- CLI HTTP/GraphQL response summaries now show status, time, and size in a compact
  table. `request get` uses tables for headers and path/query parameters, including
  disabled states, while keeping metadata and documentation as detail lines. Long
  header/parameter names and values are truncated with an ellipsis to fit the
  terminal; piped tables use a fixed 120-column limit. Scripts should use `--json`
  for full values. The `Headers:` heading is now `Headers` in `request get` and
  `request run --show-headers`; response header lines still use `name: value`.
  Path parameter names no longer have a `:` prefix, and ` [disabled]` markers
  have moved into the STATE column of request detail tables.
  `request set` now confirms the selector without repeating the entire request.
  JSON output contracts remain unchanged.

## [1.0.0] - 2026-10-13

### Changed

- CLI request, folder, and environment lists and request variable tables now align
  columns by Unicode display width, bound long values with ellipses, and adapt to
  terminal width. Piped output is now space-aligned and truncated at a deterministic
  120-column width instead of tab-separated. Scripts should use `--json`.

### Fixed

- Closing a request tab now unloads its response and cancels any running request.
  Reopening a saved request starts fresh; switching tabs still preserves responses.

## [0.11.0] - 2026-10-12

### Added

- Send runtime WebSocket binary messages with repeatable `--send-binary <BASE64>`
  arguments, interleaved in order with text `--send`, or one Base64 message per stdin
  line with `--stdin-format binary` (stdin defaults to text).
- Choose Text or Binary in the desktop WebSocket composer. Binary input uses standard
  Base64; invalid input shows an error and preserves the draft. Successful sends use
  the existing binary timeline with Base64 and byte count.

### Changed

- Desktop WebSocket disconnect lines show a registered close code with its
  display name, such as `1012 Service Restart`, and include the peer-provided
  reason when the peer sent one.

- Desktop WebSocket disconnect rows are labeled Disconnected, with a small
  severity icon before the label. The message starts at the close origin, such
  as `remote · 1012 Service Restart`. The close code and its name are colored
  by severity, using the same palette as HTTP status. The origin and the peer
  reason stay in the primary text color.

- Added hints to desktop utility icon buttons and labeled the house button “Home”.

- Polished the desktop WebSocket editor and session panel with clearer status,
  quieter utility actions, compact message rows, and an auto-hiding timeline scrollbar.

### Fixed

- Active request tab text in light mode uses a darker orange that meets WCAG AA contrast.

- WebSocket `Cmd + Enter` on macOS and `Ctrl + Enter` on Windows/Linux follow
  keyboard focus: connect from the request area, or send from the message composer.
  Disconnected composers preserve the draft and prompt you to connect first rather
  than opening a connection.

- Light-theme HTTP status text for success, redirects, and client errors, and muted text, now meet WCAG AA contrast on raised surfaces.

- Scrollbar thumbs use their own colors, so the light-theme thumb stays quiet when muted text is darkened and strengthens from rest to hover to drag.

- Variable tooltips show unavailable-environment guidance as plain text without a text cursor.

- Request editor brackets and quotes now auto-pair only in JSON mode.

- Fixed renaming, reordering, and moving GraphQL requests in directory-based
  OpenCollection collections.

### Added

- Import Yaak WebSocket requests with messages, headers, authentication, variables,
  folder placement, and ordering; report settings and fields that cannot be preserved.

- Connect native WebSocket requests in the desktop, with a saved Message editor,
  live sent/received timeline, and literal multi-line text composer. Sessions stay
  connected across tab switches; closing their tab or workspace closes them.
  History is bounded in memory, incoming binary frames display as base64, and
  configured binary messages can be edited but not executed.

- Execute native WebSocket requests with `probe request run`: interactive streaming,
  repeatable literal `--send`, inbound `--max-messages`, live-run `--timeout`, and
  NDJSON `--json`. Incoming binary frames display as base64; configured binary
  messages remain unsupported. WebSocket inspection and variable locations use
  native terminology.

- OpenCollection WebSocket requests (`info.type: websocket`) now load as native
  requests in bundled and unbundled collections. Probe keeps their URL, headers,
  authentication, message or message variants, docs, and settings when you edit
  and save them. Native WebSocket requests execute through both the CLI and desktop.

## [0.10.8] - 2026-10-09

### Added

- The desktop app reads an optional TOML user configuration file before it
  creates the main window. macOS and Linux use
  `$XDG_CONFIG_HOME/probe/config.toml` when that variable is an absolute path.
  An empty or relative value is ignored, and the file is
  `~/.config/probe/config.toml` when `HOME` is an absolute path. Windows uses
  `%APPDATA%\probe\config.toml` when `APPDATA` is an absolute path. A relative
  `HOME` or `APPDATA` is reported instead of a path under the working directory.
  A missing file keeps the built-in defaults and is not created. A symlink to a
  regular file is followed. A dangling symlink, a directory or other non-file,
  a file larger than 64 KiB, and invalid TOML are shown with the config path,
  and the window still opens using those defaults.

- The desktop `theme` setting is `system` (the default), `light`, or `dark`.
  `system` follows the OS appearance, including the macOS titlebar. `light`
  keeps the light theme and `dark` keeps the dark theme when the OS appearance
  changes, and on macOS the native window chrome follows that choice.

- `probe agent skill install` installs the bundled Probe agent skill and matching CLI
  reference locally; `--force` updates an existing installation.

### Changed

- Large workspaces retain less memory for request and folder selectors and use
  faster lookup from runtime keys to repository selectors.

## [0.10.7] - 2026-10-08

### Added

- Send requests with Cmd+Enter on macOS or Ctrl+Enter on Windows and Linux,
  with the shortcut shown in the Send button tooltip.

- Linux x86_64 releases include CLI and desktop RPM packages alongside the
  existing tarballs.

## [0.10.6] - 2026-10-07

### Changed

- The app icon now uses clean vector artwork on a light porcelain grid background,
  with vivid golden-orange colors and a brighter dark variant.
- macOS packages now include a layered native app icon with Default and Dark
  appearances selected by macOS, a graphite background for Mono/Clear styles,
  and a light fallback for older macOS versions.

## [0.10.5] - 2026-10-07

### Changed

- Redesigned the app icon with a golden-orange test probe, contact rings, and a graphite grid background.
- CLI `--description <text>` edits the content of an existing structured OpenCollection description without losing its `type`. Use `--description-json` to replace the representation explicitly.

## [0.10.4] - 2026-10-06

### Changed

- macOS CLI and desktop release artifacts are built on macOS 26 runners.

## [0.10.3] - 2026-10-06

### Changed

- Production CLI and desktop release artifacts are built with Rust 1.99.0. The minimum supported Rust version remains 1.95, with compatibility validated using Rust 1.95.0.

## [0.10.2] - 2026-10-06

### Fixed

- Long environment lists in Environment Manager scroll while keeping rows and text at their normal size.
- Variable highlights and tooltip hit areas follow manual text scrolling when the caret is offscreen.
- Shift+wheel accumulates rapid events and reverses correctly at text edges, using the field’s line height. Trackpad events without gesture phases tolerate delayed frames before releasing their axis after an idle pause.
- Scrolling over text fields in Environment Manager and request-editor lists retains the trackpad gesture axis across rows. Environment Manager reuses effective variable rows during scrolling.

## [0.10.1] - 2026-10-06

### Changed

- `probe request run` shows the effective URL sent by the HTTP engine in human output, including path/query parameters and URL normalization. Secret-derived URLs retain safe variable references; secrets used only in headers, bodies, or header authentication do not hide the effective initial URL. JSON output is unchanged.
- `probe request run` shows `Final URL:` in human output only when redirects change the effective URL initially sent. Path/query parameters and URL normalization alone do not show it. JSON output continues to include the response URL.
- `probe request run` hides response headers in human output by default. Add `--show-headers` to include them; JSON output continues to include headers.

## [0.10.0] - 2026-10-05

### Added

- Show and edit an environment description in the desktop Environment manager. The field matches the request description field: a plain string and a `{content, type}` object show their text, explicit null shows an empty field, and saving writes that same documentation value. Clearing the field writes an empty string, or an empty `content` while keeping the object's type.
- Set and unset an OpenCollection environment description from the CLI. `probe environment set --environment <name> --description <text>` writes a string, and `--description-json` writes a JSON string, null, or `{content, type}` object. `probe environment unset --environment <name> --description` removes the key and does not write null.
- `environment list --json` includes each environment's description. An environment with no description omits that field. The text table remains name and parent.
- View and edit request documentation in a Documentation tab and collection or folder documentation in reusable overview tabs, with the editor save icon and unsaved-change protection. The sidebar collection header opens its overview; folder names open overviews and disclosure chevrons expand or collapse folders.
- Show, set, and unset OpenCollection collection summary and docs, folder description and docs, and request description and docs from the CLI. Unset removes the key; `set --docs-json null` writes YAML null.
- Set, replace, and clear a request's headers, query parameters, path parameters, HTTP body, and basic, bearer, API-key, or inherit authentication with `probe request set` and `probe request create`. HTTP body writes cover the OpenCollection body types, and a body write keeps an existing variant list's titles and selection.
- Execute OpenCollection API Key authentication from a header or query parameter. Copy as cURL uses the same placement.

### Changed

- The desktop request tree, editor breadcrumbs, and tab tooltips use globe icons with bold HTTP method labels and a GraphQL icon. Request icons are sized to balance with folder icons and centered in their slots. Folder headings align with their immediate child requests; nested folders and guide lines show the hierarchy.
- The desktop authentication selector offers Inherit, Basic, Bearer, and API Key. Other OpenCollection authentication types remain in the collection file.

### Fixed

- Response pane splitters work in maximized Windows windows in both vertical and horizontal layouts.
- Request tab tooltips show custom HTTP method names alongside the generic HTTP icon.
- Saving an environment description together with other environment changes writes both in one update. A failed save does not leave the structural edit without the description, or the description without the structural edit.
- `collection unset --description-json` reports that collections have summary and docs, not a description.
- An empty collection, folder, or request update reports that the update has no changed fields.
- Vertical trackpad scrolling keeps moving the list when the pointer is over a single-line field, instead of stuttering on sideways drift.

## [0.9.5] - 2026-10-03

### Fixed

- Reduce unnecessary desktop redraws while idle and when scrolling over request fields.

## [0.9.4] - 2026-10-03

### Added

- Copy HTTP and GraphQL requests as cURL commands from the desktop Send dropdown. Plain environment variables are resolved; secret placeholders are retained. Commands use one canonical POSIX-quoted format.

### Fixed

- Avoid reloading collections for temporary files created while saving requests.
- Keep the focused Environment Manager variable field active when scrolling it out of view and back.
- Avoid unnecessary text measurements when scrolling lists over single-line fields.

## [0.9.3] - 2026-09-30

### Fixed

- Collection warnings identify unsupported YAML values and the affected item by name when available.

## [0.9.2] - 2026-09-30

### Added

- Remember tab order, the active tab, and collapsed folders separately for each workspace.

### Changed

- Newly created and imported workspaces start with empty tabs and expanded folders.

## [0.9.1] - 2026-09-29

### Changed

- Keep environment variable add buttons visible while scrolling, and scroll new variables and secrets into view.

### Fixed

- Scroll environment variables and request fields vertically even when the pointer is over a single-line input.

## [0.9.0] - 2026-09-29

### Added

- Manage secret environment variables in the desktop app, with values stored in the operating system's credential store.
- Set or replace secrets from request variable tooltips, with stored, missing, and unverified status indicators.
- Supply CLI secrets from process environment variables with `request run --secret-provider env`.
- Redact secrets from displayed requests and responses; requests using secrets bypass the response cache.

### Changed

- Improve responsiveness when editing large environment lists and updating large collections.
- Warn before renaming environments or secret variables whose credentials need to be stored again.

### Fixed

- Keep the new-environment dialog open on Linux and preserve its entered name during collection reloads.
- Prevent outdated saves from reporting success after a workspace has changed or reloaded.
- Preserve cancellation and timeout error categories for requests using secrets.

## [0.8.6] - 2026-09-26

### Added

- Remember the editor section, response tab, and Raw subview separately for each open request tab.

## [0.8.5] - 2026-09-26

### Added

- Warn about unsupported collection values when opening, importing, or validating collections, while preserving their YAML.

### Changed

- Display response sizes consistently in KiB and MiB.

### Fixed

- Preserve local GraphQL body edits when a collection reloads.
- Keep in-flight responses associated with the correct requests after reordering or reloading a collection.
- Accept CLI option values beginning with a hyphen.
- Reclaim discarded responses before rejecting a new response for exceeding the disk cache limit.

## [0.8.4] - 2026-09-24

### Fixed

- Let the response viewer fill the available height on large screens.

## [0.8.3] - 2026-09-24

### Added

- Reorder request tabs by dragging them.
- Choose a destination folder, or create one, when saving a new request.

### Changed

- New HTTP and GraphQL requests stay unsaved until you explicitly save them to a collection.
- Keep the new-tab button accessible when the tab strip scrolls.

## [0.8.2] - 2026-09-22

### Added

- Check HTTP status codes in CLI runs with `--expect status=...`; failed expectations return a nonzero exit code.
- Highlight unresolved environment and URL path placeholders in the request editor.

### Changed

- Improve automatic quote and bracket pairing, including skipping existing closing characters and deleting empty pairs.

### Fixed

- Select a default environment after importing a Yaak workspace when one is available.

## [0.8.1] - 2026-09-20

### Added

- Preview a resolved CLI request without sending it using `request run --dry-run`.
- Copy JSON keys and scalar values, and XML text and attribute values, from the response context menu.
- Automatically pair quotes and brackets in multiline request editors.

### Fixed

- Make Tab and Shift+Tab indent and outdent multiline request editors; Ctrl+Tab and Ctrl+Shift+Tab move focus between controls.
- Make disabled context-menu items readable.

## [0.8.0] - 2026-09-13

### Added

- Create, edit, save, and run GraphQL requests in the desktop app and CLI, using GET or POST.
- Edit GraphQL queries, variables, operation names, and extensions.
- Import GraphQL requests from Postman and Yaak exports.

## [0.7.1] - 2026-09-11

### Changed

- Adjust response viewer spacing around headers, tabs, and content.

## [0.7.0] - 2026-09-11

### Added

- Inspect response bytes in a Hex view under Raw, including large responses.

### Changed

- Binary responses open in Hex view by default.

## [0.6.2] - 2026-09-08

### Fixed

- Preserve the caret position and undo history while editing request bodies.

## [0.6.1] - 2026-09-06

### Changed

- Combine Send and its options into a single dropdown button.

## [0.6.0] - 2026-09-06

### Added

- Preview image responses, including images recognized from their contents.
- Show request progress while a response downloads.
- Download response bodies or send a request directly to a file.

### Changed

- Improve response viewer responsiveness.

### Fixed

- Clear the response when discarding a request tab's changes.

## [0.5.7] - 2026-09-03

### Added

- Display the CLI version with `--version` or `-V`, including JSON and quiet output modes.

## [0.5.6] - 2026-09-02

### Added

- Remove entries from the recent collections list.

### Fixed

- Correct recent collection display, folder selection, and focus indicators.

## [0.5.5] - 2026-09-02

### Added

- Reject unresolved CLI variables with `--strict-variables` on `request get` and `request run`.

### Fixed

- Keep open request tabs and local edits associated with a request after it is renamed.
- Correct sidebar folder expansion behavior.

## [0.5.4] - 2026-09-02

### Changed

- Reveal the active request in the sidebar when switching tabs or restoring a session.
- Automatically expand matching folders during sidebar searches.

## [0.5.3] - 2026-09-02

### Changed

- Search requests by their folder path as well as their name, and include descendants of matching folders.

## [0.5.2] - 2026-08-31

No notable user-facing changes.

## [0.5.1] - 2026-08-31

### Added

- List a request's referenced variables and their locations with `request variables`, including environment availability and secret status.

## [0.5.0] - 2026-08-31

### Added

- Supply temporary CLI request variables with repeatable `--var name=value` options without modifying the collection.

## [0.4.2] - 2026-08-29

### Changed

- Simplify the response viewer layout and update pane dividers.

## [0.4.1] - 2026-08-28

### Added

- Open the Environment Manager directly from a request variable tooltip.

## [0.4.0] - 2026-08-28

### Added

- Manage environments and their variables in a desktop Environment Manager, with the save shortcut supported.
- Rename and delete environments from the CLI.
- Show operation notifications and error banners in dialogs.

## [0.3.0] - 2026-08-27

### Added

- Format, highlight, and inspect XML responses.
- View raw response bodies as Text or Base64.
- Browse large responses in pages, with bounded disk storage and cleanup after crashes.

### Changed

- Increase the in-memory response limit to 16 MiB and support highlighting larger responses.
- Show raw response text without soft wrapping.

## [0.2.2] - 2026-08-27

### Fixed

- Prevent an extra console window from appearing when launching the desktop app on Windows.

## [0.2.1] - 2026-08-26

### Added

- Show request tab tooltips on hover.
- Zoom the window by double-clicking the title bar.

## [0.2.0] - 2026-08-26

### Added

- Decode JWTs and inspect Unix timestamps in JSON responses, with links to their locations in the Pretty view.
- Search response text through a dedicated search panel.

### Fixed

- Correct response search highlighting.
- Restore save shortcuts and keyboard focus behavior in the request tree.
- Correct desktop window lifecycle behavior on macOS.
- Ignore unnamed request parameters when sending requests.

## [0.1.1] - 2026-08-24

### Added

- Desktop downloads for macOS, Windows, and Linux.
- A desktop request editor with tabs, environment selection, response viewing and search, and collection management.

## [0.1.0] - 2026-08-24

### Added

- Initial CLI release for macOS, Windows, and Linux, with human-readable and JSON output.
- Open, validate, edit, and run requests from bundled and unbundled OpenCollection YAML collections.
- Manage requests, folders, environments, and variables, and save response bodies to files.
- Import Postman collections and Yaak workspaces.

[2.1.1]: https://github.com/crizant/probe/releases/tag/v2.1.1
[2.1.0]: https://github.com/crizant/probe/releases/tag/v2.1.0
[2.0.0]: https://github.com/crizant/probe/releases/tag/v2.0.0
[1.1.1]: https://github.com/crizant/probe/releases/tag/v1.1.1
[1.1.0]: https://github.com/crizant/probe/releases/tag/v1.1.0
[1.0.1]: https://github.com/crizant/probe/releases/tag/v1.0.1
[1.0.0]: https://github.com/crizant/probe/releases/tag/v1.0.0
[0.11.0]: https://github.com/crizant/probe/releases/tag/v0.11.0
[0.10.8]: https://github.com/crizant/probe/releases/tag/v0.10.8
[0.10.7]: https://github.com/crizant/probe/releases/tag/v0.10.7
[0.10.6]: https://github.com/crizant/probe/releases/tag/v0.10.6
[0.10.5]: https://github.com/crizant/probe/releases/tag/v0.10.5
[0.10.4]: https://github.com/crizant/probe/releases/tag/v0.10.4
[0.10.3]: https://github.com/crizant/probe/releases/tag/v0.10.3
[0.10.2]: https://github.com/crizant/probe/releases/tag/v0.10.2
[0.10.1]: https://github.com/crizant/probe/releases/tag/v0.10.1
[0.10.0]: https://github.com/crizant/probe/releases/tag/v0.10.0
[0.9.5]: https://github.com/crizant/probe/releases/tag/v0.9.5
[0.9.4]: https://github.com/crizant/probe/releases/tag/v0.9.4
[0.9.3]: https://github.com/crizant/probe/releases/tag/v0.9.3
[0.9.2]: https://github.com/crizant/probe/releases/tag/v0.9.2
[0.9.1]: https://github.com/crizant/probe/releases/tag/v0.9.1
[0.9.0]: https://github.com/crizant/probe/releases/tag/v0.9.0
[0.8.6]: https://github.com/crizant/probe/releases/tag/v0.8.6
[0.8.5]: https://github.com/crizant/probe/releases/tag/v0.8.5
[0.8.4]: https://github.com/crizant/probe/releases/tag/v0.8.4
[0.8.3]: https://github.com/crizant/probe/releases/tag/v0.8.3
[0.8.2]: https://github.com/crizant/probe/releases/tag/v0.8.2
[0.8.1]: https://github.com/crizant/probe/releases/tag/v0.8.1
[0.8.0]: https://github.com/crizant/probe/releases/tag/v0.8.0
[0.7.1]: https://github.com/crizant/probe/releases/tag/v0.7.1
[0.7.0]: https://github.com/crizant/probe/releases/tag/v0.7.0
[0.6.2]: https://github.com/crizant/probe/releases/tag/v0.6.2
[0.6.1]: https://github.com/crizant/probe/releases/tag/v0.6.1
[0.6.0]: https://github.com/crizant/probe/releases/tag/v0.6.0
[0.5.7]: https://github.com/crizant/probe/releases/tag/v0.5.7
[0.5.6]: https://github.com/crizant/probe/releases/tag/v0.5.6
[0.5.5]: https://github.com/crizant/probe/releases/tag/v0.5.5
[0.5.4]: https://github.com/crizant/probe/releases/tag/v0.5.4
[0.5.3]: https://github.com/crizant/probe/releases/tag/v0.5.3
[0.5.2]: https://github.com/crizant/probe/releases/tag/v0.5.2
[0.5.1]: https://github.com/crizant/probe/releases/tag/v0.5.1
[0.5.0]: https://github.com/crizant/probe/releases/tag/v0.5.0
[0.4.2]: https://github.com/crizant/probe/releases/tag/v0.4.2
[0.4.1]: https://github.com/crizant/probe/releases/tag/v0.4.1
[0.4.0]: https://github.com/crizant/probe/releases/tag/v0.4.0
[0.3.0]: https://github.com/crizant/probe/releases/tag/v0.3.0
[0.2.2]: https://github.com/crizant/probe/releases/tag/v0.2.2
[0.2.1]: https://github.com/crizant/probe/releases/tag/v0.2.1
[0.2.0]: https://github.com/crizant/probe/releases/tag/v0.2.0
[0.1.1]: https://github.com/crizant/probe/releases/tag/v0.1.1
[0.1.0]: https://github.com/crizant/probe/releases/tag/v0.1.0
