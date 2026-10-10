//! Native OpenCollection item types shared by projection, loading, and structural edits.

use probe_core::RequestProtocol;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeItemType {
    Folder,
    Request(RequestProtocol),
}

impl NativeItemType {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        if name == "folder" {
            Some(Self::Folder)
        } else {
            RequestProtocol::from_name(name).map(Self::Request)
        }
    }
}
