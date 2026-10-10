//! Native OpenCollection item types shared by projection, loading, and structural edits.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeItemType {
    Folder,
    Http,
    Graphql,
    WebSocket,
}

impl NativeItemType {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "folder" => Some(Self::Folder),
            "http" => Some(Self::Http),
            "graphql" => Some(Self::Graphql),
            "websocket" => Some(Self::WebSocket),
            _ => None,
        }
    }
}
