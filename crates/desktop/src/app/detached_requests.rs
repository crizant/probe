use std::collections::{BTreeMap, BTreeSet};

use probe_core::RequestKey;

/// Tracks unsaved requests and the subset whose write committed but refresh failed.
/// A committed request must not be written again until the workspace is reloaded.
#[derive(Default)]
pub(super) struct DetachedRequests {
    keys: BTreeSet<RequestKey>,
    committed: BTreeSet<RequestKey>,
}

impl DetachedRequests {
    pub(super) fn contains(&self, key: &RequestKey) -> bool {
        self.keys.contains(key)
    }
    pub(super) fn is_committed(&self, key: &RequestKey) -> bool {
        self.committed.contains(key)
    }
    pub(super) fn iter(&self) -> impl Iterator<Item = &RequestKey> {
        self.keys.iter()
    }
    pub(super) fn insert(&mut self, key: RequestKey) {
        self.keys.insert(key);
    }
    pub(super) fn remove(&mut self, key: &RequestKey) -> bool {
        self.committed.remove(key);
        self.keys.remove(key)
    }
    pub(super) fn clear(&mut self) {
        self.keys.clear();
        self.committed.clear();
    }
    pub(super) fn mark_committed(&mut self, key: RequestKey) {
        if self.keys.contains(&key) {
            self.committed.insert(key);
        }
    }
    pub(super) fn remap(&mut self, remaps: &BTreeMap<RequestKey, RequestKey>) {
        self.keys = self
            .keys
            .iter()
            .filter_map(|key| remaps.get(key).copied())
            .collect();
        self.committed = self
            .committed
            .iter()
            .filter_map(|key| remaps.get(key).copied())
            .filter(|key| self.keys.contains(key))
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use probe_core::{Collection, Request, Workspace};

    #[test]
    fn committed_state_stays_with_detached_request_across_reload() {
        let mut old = Workspace::from_collection(Collection::default());
        let old_key = old.add_detached_request(Request::default());
        let mut new = Workspace::from_collection(Collection::default());
        let new_key = new.add_detached_request(Request::default());
        let mut state = DetachedRequests::default();
        state.mark_committed(old_key);
        assert!(!state.is_committed(&old_key));
        state.insert(old_key);
        state.mark_committed(old_key);
        state.remap(&[(old_key, new_key)].into());
        assert!(!state.contains(&old_key));
        assert!(state.contains(&new_key));
        assert!(state.is_committed(&new_key));
        state.remove(&new_key);
        assert!(!state.is_committed(&new_key));
    }
}
