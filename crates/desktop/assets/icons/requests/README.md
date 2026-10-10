# Request icons

Original Probe SVG artwork for request navigation. HTTP method icons share a
48×32 viewBox: an interrupted globe surrounds the full method name. The bold condensed letters
are filled vector paths, so the assets need no installed font or embedded font data.
All methods use the same lettering size and canvas, including OPTIONS and CONNECT.
The globe uses 55% opacity and a wider central gap to give the lettering priority.

HTTP, GraphQL, and WebSocket protocol icons retain their 24×24 viewBox.
All assets use `currentColor` for tinting and contain no external dependencies.

| Asset | Label |
| --- | --- |
| [get.svg](get.svg) | GET |
| [post.svg](post.svg) | POST |
| [put.svg](put.svg) | PUT |
| [patch.svg](patch.svg) | PATCH |
| [delete.svg](delete.svg) | DELETE |
| [head.svg](head.svg) | HEAD |
| [options.svg](options.svg) | OPTIONS |
| [connect.svg](connect.svg) | CONNECT |
| [trace.svg](trace.svg) | TRACE |
| [http.svg](http.svg) | HTTP |
| [graphql.svg](graphql.svg) | GraphQL |
| [websocket.svg](websocket.svg) | WebSocket |

[preview.svg](preview.svg) shows both themes. Method icons are displayed at 60×40
and 30×20; protocol icons at 40×40 and 20×20. The desktop uses a shared 20×20 icon slot. Folders have a 24px disclosure gutter,
matching one nesting level; requests omit that gutter to align with their parent
folder heading.
Method artwork is centered at 24×16 inside the slot; protocol icons use 16×16. Avoid squeezing the method artwork into a
16×16 square: its lettering needs a wider slot. Confirm readability in GPUI before
integrating. The preview uses existing method/protocol palette values; WebSocket
borrows the HTTP color for illustration because no WebSocket token exists yet.

Reuse the existing GPUI SVG rendering helper with embedded bytes and a stable,
unique cache key per icon. Render the method SVGs with their 3:2 aspect ratio;
the desktop SVG helper supports rectangular method icons.
Use `http.svg` as the fallback for custom methods. Keep the full method/protocol
in the row accessibility label and tooltip. The tree, drag previews, and add menu use these assets. The WebSocket icon is
reserved for future use; it does not enable WebSocket support.
