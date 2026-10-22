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

pub(crate) fn summary(results: &[AssertionResult]) -> Vec<String> {
    if results.is_empty() {
        return Vec::new();
    }
    let [failed, errors, passed, skipped] = counts(results);
    let error_text = || format!("{errors} {}", if errors == 1 { "error" } else { "errors" });
    let mut parts = vec![if failed > 0 {
        format!("{failed} of {} failed", failed + passed + errors)
    } else if errors > 0 {
        error_text()
    } else if passed > 0 {
        format!("{passed} passed")
    } else {
        "All assertions disabled".into()
    }];
    if failed == 0 && errors > 0 && passed > 0 {
        parts.push(format!("{passed} passed"));
    }
    if skipped > 0 && skipped != results.len() {
        parts.push(format!("{skipped} skipped"));
    }
    if failed > 0 && errors > 0 {
        parts.push(error_text());
    }
    parts
}

pub(crate) fn single_line_actual(actual: &str) -> String {
    let encoded = serde_json::to_string(actual).expect("display text is serializable");
    encoded[1..encoded.len() - 1].to_owned()
}

pub(crate) fn copy_row(declaration: &Assertion, result: &AssertionResult) -> String {
    let mut text = format!("{} {}", declaration.expression, declaration.operator);
    if !matches!(declaration.operator.as_str(), "isNull" | "isNotNull")
        && let Some(value) = declaration
            .value
            .as_deref()
            .filter(|value| !value.is_empty())
    {
        text.push(' ');
        text.push_str(value);
    }
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
    fn results_remain_stably_ordered_by_outcome() {
        use AssertionOutcome::{Disabled, Error, Failed, Passed};
        let results = results(vec![
            Passed,
            Failed,
            Disabled,
            Error(AssertionError::BodyUnavailable),
            Failed,
            Passed,
        ]);
        assert_eq!(
            ordered(&results)
                .iter()
                .map(|r| r.index)
                .collect::<Vec<_>>(),
            [1, 4, 3, 0, 5, 2]
        );
    }

    #[test]
    fn actual_text_escapes_control_characters() {
        assert_eq!(
            single_line_actual("first\nsecond\tend"),
            "first\\nsecond\\tend"
        );
    }

    fn results(outcomes: Vec<AssertionOutcome>) -> Vec<AssertionResult> {
        outcomes
            .into_iter()
            .enumerate()
            .map(|(index, outcome)| AssertionResult {
                index,
                outcome,
                actual: None,
                actual_truncated: false,
            })
            .collect()
    }

    #[test]
    fn summary_covers_each_execution_state() {
        use AssertionOutcome::{Disabled, Error, Failed, Passed};
        let error = Error(AssertionError::BodyUnavailable);
        for (outcomes, expected) in [
            (vec![Failed, Passed], vec!["1 of 2 failed"]),
            (
                vec![Passed, Failed, Disabled, error.clone(), Failed, Passed],
                vec!["2 of 5 failed", "1 skipped", "1 error"],
            ),
            (
                vec![Failed, Disabled, Disabled],
                vec!["1 of 1 failed", "2 skipped"],
            ),
            (vec![error.clone()], vec!["1 error"]),
            (vec![error.clone(), Disabled], vec!["1 error", "1 skipped"]),
            (
                vec![error.clone(), error.clone(), Passed, Disabled],
                vec!["2 errors", "1 passed", "1 skipped"],
            ),
            (vec![Passed], vec!["1 passed"]),
            (vec![Passed, Passed], vec!["2 passed"]),
            (vec![Passed, Disabled], vec!["1 passed", "1 skipped"]),
            (vec![Disabled, Disabled], vec!["All assertions disabled"]),
            (
                vec![Failed, error, Disabled],
                vec!["1 of 2 failed", "1 skipped", "1 error"],
            ),
            (vec![], vec![]),
        ] {
            assert_eq!(summary(&results(outcomes)), expected);
        }
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    #[test]
    fn copied_rows_omit_unused_or_absent_expected_values() {
        let mut declaration = Assertion {
            expression: "response.body.item".into(),
            operator: String::new(),
            value: Some("preserved unused value".into()),
            disabled: None,
            description: None,
        };
        let mut result = AssertionResult {
            index: 0,
            outcome: AssertionOutcome::Passed,
            actual: Some("null".into()),
            actual_truncated: false,
        };
        for operator in ["isNull", "isNotNull"] {
            declaration.operator = operator.into();
            assert_eq!(
                copy_row(&declaration, &result),
                format!("response.body.item {operator}")
            );
        }
        declaration.operator = "equals".into();
        for value in [None, Some(String::new())] {
            declaration.value = value;
            assert_eq!(copy_row(&declaration, &result), "response.body.item equals");
        }
        result.outcome = AssertionOutcome::Failed;
        assert_eq!(
            copy_row(&declaration, &result),
            "response.body.item equals\ngot null"
        );
        result.actual = None;
        assert_eq!(copy_row(&declaration, &result), "response.body.item equals");
    }

    #[test]
    fn copy_includes_only_failed_actuals() {
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
