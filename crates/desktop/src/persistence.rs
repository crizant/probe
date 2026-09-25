use std::collections::{BTreeMap, VecDeque};

use probe_core::{GraphqlRequestError, HttpRequest, RequestKey, RequestUpdate};

#[derive(Debug, Default)]
pub(crate) struct PersistenceState {
    saved: BTreeMap<RequestKey, HttpRequest>,
    revisions: BTreeMap<RequestKey, u64>,
    saving: BTreeMap<RequestKey, u64>,
    queue: VecDeque<RequestKey>,
}

impl PersistenceState {
    pub(crate) fn reset(&mut self, requests: impl IntoIterator<Item = (RequestKey, HttpRequest)>) {
        self.saved = requests.into_iter().collect();
        self.revisions = self.saved.keys().map(|key| (*key, 0)).collect();
        self.saving.clear();
        self.queue.clear();
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn saved_request(&self, key: RequestKey) -> Option<&HttpRequest> {
        self.saved.get(&key)
    }

    pub(crate) fn edited(&mut self, key: RequestKey) {
        *self.revisions.entry(key).or_default() += 1;
    }

    pub(crate) fn is_dirty(&self, key: RequestKey, request: &HttpRequest) -> bool {
        self.saved.get(&key) != Some(request)
            || self.saving.get(&key).is_some_and(|revision| {
                self.revisions
                    .get(&key)
                    .is_some_and(|current| current > revision)
            })
    }

    pub(crate) fn dirty_keys<'a>(
        &'a self,
        requests: impl IntoIterator<Item = (RequestKey, &'a HttpRequest)> + 'a,
    ) -> Vec<RequestKey> {
        requests
            .into_iter()
            .filter_map(|(key, request)| self.is_dirty(key, request).then_some(key))
            .collect()
    }

    pub(crate) fn enqueue(&mut self, keys: impl IntoIterator<Item = RequestKey>) {
        for key in keys {
            if !self.queue.contains(&key) {
                self.queue.push_back(key);
            }
        }
    }

    pub(crate) fn next(&mut self) -> Option<RequestKey> {
        let key = self.queue.pop_front()?;
        self.saving
            .insert(key, self.revisions.get(&key).copied().unwrap_or_default());
        Some(key)
    }

    pub(crate) fn has_outstanding_saves(&self) -> bool {
        !self.queue.is_empty() || !self.saving.is_empty()
    }

    pub(crate) fn begin(
        &self,
        key: RequestKey,
        request: &HttpRequest,
    ) -> Result<(u64, HttpRequest, RequestUpdate), GraphqlRequestError> {
        let snapshot = request.clone();
        let update = RequestUpdate::between(self.saved.get(&key), &snapshot)?;
        Ok((
            self.revisions.get(&key).copied().unwrap_or_default(),
            snapshot,
            update,
        ))
    }

    pub(crate) fn complete(&mut self, key: RequestKey, snapshot: HttpRequest) {
        self.saving.remove(&key);
        self.saved.insert(key, snapshot);
    }

    pub(crate) fn fail(&mut self, key: RequestKey) {
        self.saving.remove(&key);
        self.queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use probe_core::{
        Collection, CollectionItem, GraphqlBody, GraphqlBodyVariant, GraphqlOperation,
        GraphqlRequest, GraphqlRequestError, HttpRequest, Workspace, WorkspaceItemRef,
    };

    use super::PersistenceState;

    fn request_key() -> probe_core::RequestKey {
        let workspace = Workspace::from_collection(Collection {
            items: vec![CollectionItem::HttpRequest(HttpRequest::default())],
            ..Collection::default()
        });
        let [WorkspaceItemRef::Request(key)] = workspace.root_items() else {
            unreachable!()
        };
        *key
    }

    fn graphql_request(body: GraphqlBody) -> HttpRequest {
        GraphqlRequest {
            body: Some(body),
            ..GraphqlRequest::default()
        }
        .into_request()
    }

    fn variants(first_selected: bool, second_selected: bool) -> GraphqlBody {
        GraphqlBody::Variants(vec![
            GraphqlBodyVariant {
                title: "first".to_owned(),
                selected: first_selected,
                body: GraphqlOperation {
                    query: Some("query First { first }".to_owned()),
                    ..GraphqlOperation::default()
                },
            },
            GraphqlBodyVariant {
                title: "second".to_owned(),
                selected: second_selected,
                body: GraphqlOperation {
                    query: Some("query Second { second }".to_owned()),
                    ..GraphqlOperation::default()
                },
            },
        ])
    }

    #[test]
    fn completion_tracks_the_saved_snapshot_not_newer_edits() {
        let key = request_key();
        let original = HttpRequest::default();
        let mut state = PersistenceState::default();
        state.reset([(key, original.clone())]);

        let mut request = original;
        request.url = Some("https://saved.example".to_owned());
        state.edited(key);
        let (_, saved_snapshot, _) = state.begin(key, &request).unwrap();
        request.url = Some("https://newer.example".to_owned());
        state.edited(key);
        state.complete(key, saved_snapshot);

        assert!(state.is_dirty(key, &request));
    }

    #[test]
    fn enqueue_keeps_a_follow_up_save_for_a_request_that_is_already_saving() {
        let key = request_key();
        let mut state = PersistenceState::default();
        state.reset([(key, HttpRequest::default())]);

        state.enqueue([key]);
        assert_eq!(state.next(), Some(key));
        state.enqueue([key]);
        state.complete(key, HttpRequest::default());

        assert_eq!(state.next(), Some(key));
    }

    #[test]
    fn invalid_graphql_selection_rejects_diff_without_advancing_saved_baseline() {
        let key = request_key();
        for (body, message) in [
            (variants(false, false), "no selected value"),
            (variants(true, true), "multiple selected values"),
        ] {
            let baseline = graphql_request(variants(true, false));
            let draft = graphql_request(body);
            let mut state = PersistenceState::default();
            state.reset([(key, baseline.clone())]);
            state.edited(key);
            state.enqueue([key]);
            assert_eq!(state.next(), Some(key));
            assert!(
                matches!(state.begin(key, &draft), Err(GraphqlRequestError::InvalidBodySelection(error)) if error.contains(message))
            );
            state.fail(key);
            assert!(state.is_dirty(key, &draft));
            assert_eq!(state.saved_request(key), Some(&baseline));
            assert!(!state.has_outstanding_saves());
        }
    }
}
