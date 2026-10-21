//! Presentation rules for an immutable execution's assertion results.
use probe_application::{AssertionOutcome, AssertionResult};
use probe_core::Assertion;

pub(crate) fn counts(results: &[AssertionResult]) -> [usize; 4] {
    let mut counts = [0; 4];
    for result in results {
        counts[rank(&result.outcome)] += 1;
    }
    counts
}

fn rank(outcome: &AssertionOutcome) -> usize {
    match outcome {
        AssertionOutcome::Failed => 0,
        AssertionOutcome::Error(_) => 1,
        AssertionOutcome::Passed => 2,
        AssertionOutcome::Disabled => 3,
    }
}

pub(crate) fn ordered(results: &[AssertionResult]) -> Vec<&AssertionResult> {
    let mut rows: Vec<_> = results.iter().collect();
    rows.sort_by_key(|result| rank(&result.outcome));
    rows
}

pub(crate) fn summary(results: &[AssertionResult]) -> String {
    let [failed, errors, passed, skipped] = counts(results);
    let mut text = if failed > 0 {
        format!("{failed} of {} failed", failed + passed + errors)
    } else {
        format!("{passed} passed")
    };
    if skipped > 0 {
        text.push_str(&format!(" · {skipped} skipped"));
    }
    if errors > 0 {
        text.push_str(&format!(
            " · {errors} {}",
            if errors == 1 { "error" } else { "errors" }
        ));
    }
    text
}

pub(crate) fn single_line_actual(actual: &str) -> String {
    let encoded = serde_json::to_string(actual).expect("display text is serializable");
    encoded[1..encoded.len() - 1].to_owned()
}

pub(crate) fn copy_row(declaration: &Assertion, result: &AssertionResult) -> String {
    let mut text = format!(
        "{} {} {}",
        declaration.expression,
        declaration.operator,
        declaration.value.as_deref().unwrap_or_default()
    );
    if result.outcome == AssertionOutcome::Failed
        && let Some(actual) = &result.actual
    {
        text.push_str(&format!("\ngot {actual}"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use probe_application::AssertionError;
    #[test]
    fn summary_and_order_distinguish_failures_errors_and_skipped() {
        let results: Vec<_> = [
            AssertionOutcome::Passed,
            AssertionOutcome::Failed,
            AssertionOutcome::Disabled,
            AssertionOutcome::Error(AssertionError::BodyUnavailable),
            AssertionOutcome::Failed,
            AssertionOutcome::Passed,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, outcome)| AssertionResult {
            index,
            outcome,
            actual: None,
            actual_truncated: false,
        })
        .collect();
        assert_eq!(summary(&results), "2 of 5 failed · 1 skipped · 1 error");
        assert_eq!(
            ordered(&results)
                .iter()
                .map(|r| r.index)
                .collect::<Vec<_>>(),
            [1, 4, 3, 0, 5, 2]
        );
        assert_eq!(summary(&results[..1]), "1 passed");
        assert_eq!(
            single_line_actual("first\nsecond\tend"),
            "first\\nsecond\\tend"
        );
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use crate::response_viewer::ResponseViewerState;
    #[test]
    fn keyboard_selection_clamps_in_display_order_and_copy_includes_only_failed_actuals() {
        let workspace = probe_core::Workspace::from_collection(probe_core::Collection {
            items: vec![probe_core::CollectionItem::Request(
                probe_core::Request::default(),
            )],
            ..Default::default()
        });
        let probe_core::WorkspaceItemRef::Request(key) = workspace.root_items()[0] else {
            panic!("request")
        };
        let mut state = ResponseViewerState::default();
        state.move_assertion(key, &[2, 0, 1], 1);
        assert_eq!(state.assertion_selection(key), Some(0));
        state.move_assertion(key, &[2, 0, 1], isize::MAX);
        assert_eq!(state.assertion_selection(key), Some(1));
        state.move_assertion(key, &[2, 0, 1], isize::MIN);
        assert_eq!(state.assertion_selection(key), Some(2));
        state.move_assertion(key, &[], 1);
        assert_eq!(state.assertion_selection(key), Some(2));
        state.remove(key);
        assert_eq!(state.assertion_selection(key), None);
        let declaration = Assertion {
            expression: "response.status".into(),
            operator: "equals".into(),
            value: Some("200".into()),
            disabled: None,
            description: None,
        };
        let mut result = AssertionResult {
            index: 0,
            outcome: AssertionOutcome::Failed,
            actual: Some("401".into()),
            actual_truncated: false,
        };
        assert_eq!(
            copy_row(&declaration, &result),
            "response.status equals 200\ngot 401"
        );
        result.outcome = AssertionOutcome::Passed;
        assert_eq!(
            copy_row(&declaration, &result),
            "response.status equals 200"
        );
    }
}
