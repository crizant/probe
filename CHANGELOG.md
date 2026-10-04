# Changelog

User-facing changes in Probe, newest release first. This changelog follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Release dates use Australia/Brisbane time (UTC+10).

## [Unreleased]

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
