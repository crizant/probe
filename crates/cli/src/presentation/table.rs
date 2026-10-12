//! One-line human tables. JSON and detail views never pass through this module.

use std::io::{IsTerminal, stdout};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const FALLBACK_WIDTH: usize = 120;

#[derive(Clone, Copy)]
enum Truncation {
    End,
    Middle,
}

pub(crate) struct Column {
    header: &'static str,
    maximum: usize,
    compact: bool,
    truncation: Truncation,
}

impl Column {
    pub(crate) const fn text(header: &'static str, maximum: usize) -> Self {
        Self {
            header,
            maximum,
            compact: false,
            truncation: Truncation::End,
        }
    }

    pub(crate) const fn selector(header: &'static str, maximum: usize) -> Self {
        Self {
            truncation: Truncation::Middle,
            ..Self::text(header, maximum)
        }
    }

    pub(crate) const fn compact(header: &'static str) -> Self {
        Self {
            compact: true,
            ..Self::text(header, 12)
        }
    }

    pub(crate) const fn remaining(header: &'static str) -> Self {
        Self::text(header, usize::MAX)
    }
}

pub(crate) fn terminal_width() -> Option<usize> {
    // Redirected output must be deterministic, even when stderr has a terminal.
    if !stdout().is_terminal() {
        return None;
    }
    terminal_size::terminal_size().map(|(terminal_size::Width(width), _)| usize::from(width))
}

pub(crate) fn render<const N: usize>(
    columns: &[Column; N],
    rows: &[[String; N]],
    terminal_width: Option<usize>,
) -> String {
    if N == 0 {
        return String::new();
    }
    let limit = terminal_width.unwrap_or(FALLBACK_WIDTH);
    let gap = if N > 1 {
        2.min(limit.saturating_sub(N) / (N - 1))
    } else {
        0
    };
    let budget = limit.saturating_sub(gap * (N - 1));
    // User-provided YAML may contain line breaks or tabs. Never let these create
    // extra rows, tab stops, or terminal controls. Probe adds no ANSI table styling.
    let rows: Vec<[String; N]> = rows
        .iter()
        .map(|row| {
            row.each_ref().map(|cell| {
                cell.chars()
                    .map(|ch| if ch.is_control() { ' ' } else { ch })
                    .collect()
            })
        })
        .collect();
    let mut widths: [usize; N] = std::array::from_fn(|index| {
        rows.iter()
            .map(|row| row[index].width())
            .chain(std::iter::once(columns[index].header.width()))
            .max()
            .unwrap_or(0)
            .min(columns[index].maximum)
            .min(budget)
    });
    // Preserve compact data first, then flexible headers, then shrink any column
    // still necessary. Saturating budgets also handle zero-width terminals.
    for phase in 0..3 {
        for _ in 0..widths.iter().sum::<usize>().saturating_sub(budget) {
            let candidate = (0..N)
                .filter_map(|index| {
                    let column = &columns[index];
                    let floor = if phase == 0 {
                        column.header.width().min(widths[index])
                    } else {
                        0
                    };
                    if (phase < 2 && column.compact) || widths[index] <= floor {
                        None
                    } else {
                        Some((index, widths[index] - floor))
                    }
                })
                .max_by_key(|&(_, slack)| slack);
            let Some((index, _)) = candidate else { break };
            widths[index] -= 1;
        }
    }
    let headers = columns.each_ref().map(|column| column.header.to_owned());
    let mut output = String::new();
    for row in std::iter::once(&headers).chain(rows.iter()) {
        let mut line = String::new();
        for index in 0..N {
            let cell = truncate(&row[index], widths[index], columns[index].truncation);
            line.push_str(&cell);
            // Pad uniformly; the row's final trim removes unused trailing cells.
            line.push_str(&" ".repeat(widths[index] - cell.width() + gap));
        }
        output.push_str(line.trim_end());
        output.push('\n');
    }
    output
}

fn truncate(value: &str, width: usize, policy: Truncation) -> String {
    if value.width() <= width {
        return value.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let available = width - 1; // The ellipsis occupies one display cell.
    let (prefix_budget, suffix_budget) = match policy {
        Truncation::End => (available, 0),
        Truncation::Middle => (available.div_ceil(2), available / 2),
    };
    let mut prefix = String::new();
    let mut used = 0;
    for grapheme in value.graphemes(true) {
        if used + grapheme.width() > prefix_budget {
            break;
        }
        prefix.push_str(grapheme);
        used += grapheme.width();
    }
    let mut suffix = Vec::new();
    used = 0;
    if suffix_budget > 0 {
        for grapheme in value.graphemes(true).rev() {
            if used + grapheme.width() > suffix_budget {
                break;
            }
            suffix.push(grapheme);
            used += grapheme.width();
        }
    }
    prefix.push('…');
    for grapheme in suffix.into_iter().rev() {
        prefix.push_str(grapheme);
    }
    prefix
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row<const N: usize>(cells: [&str; N]) -> [String; N] {
        cells.map(str::to_owned)
    }

    #[test]
    fn normal_and_wide_values_align_by_display_cells() {
        let columns = [Column::text("NAME", 32), Column::remaining("URL")];
        let rows = [
            row(["Login", "/one"]),
            row(["用戶登入", "/two"]),
            row(["測試 API", "/three"]),
        ];
        let table = render(&columns, &rows, Some(80));
        assert_eq!(
            table,
            "NAME      URL\nLogin     /one\n用戶登入  /two\n測試 API  /three\n"
        );
        for line in table.lines().skip(1) {
            assert_eq!(line[..line.find('/').unwrap()].width(), 10);
        }
    }

    #[test]
    fn caps_names_and_truncates_trailing_values_without_wrapping() {
        let columns = [Column::text("NAME", 8), Column::remaining("URL")];
        let rows = [
            row([
                "abcdefghijklmnopqrstuvwxyz",
                "https://example.com/very/long/path",
            ]),
            row(["short", "/ok"]),
        ];
        assert_eq!(
            render(&columns, &rows, Some(24)),
            "NAME      URL\nabcdefg…  https://examp…\nshort     /ok\n"
        );
        let wide = render(&columns, &rows, Some(200));
        assert!(wide.contains("abcdefg…  https://example.com/very/long/path"));
    }

    #[test]
    fn selectors_keep_both_ends() {
        let columns = [Column::selector("SELECTOR", 19)];
        assert_eq!(
            render(
                &columns,
                &[row(["items/123/items/456/items/999"])],
                Some(80)
            ),
            "SELECTOR\nitems/123…items/999\n"
        );
    }

    #[test]
    fn truncation_preserves_graphemes_and_display_bounds() {
        assert_eq!(truncate("用戶登入", 6, Truncation::End), "用戶…");
        assert_eq!(truncate("e\u{301}cole", 3, Truncation::End), "e\u{301}c…");
        assert_eq!(truncate("👩‍💻abcd", 3, Truncation::End), "👩‍💻…");
        assert_eq!(truncate("abcdef", 1, Truncation::Middle), "…");
        assert_eq!(truncate("abcdef", 2, Truncation::Middle), "a…");
        assert_eq!(truncate("用戶登入", 5, Truncation::Middle), "用…入");
        assert_eq!(truncate("exact", 5, Truncation::End), "exact");
        assert_eq!(truncate("abcdef", 0, Truncation::End), "");
        assert_eq!(truncate("abcd\u{200b}", 3, Truncation::End), "ab…");
    }

    #[test]
    fn narrow_layout_preserves_compact_columns_and_headers_when_possible() {
        let columns = [
            Column::text("NAME", 32),
            Column::compact("DEFINED"),
            Column::compact("SECRET"),
            Column::remaining("USED IN"),
        ];
        let rows = [row([
            "very long variable name",
            "true",
            "false",
            "header: Authorization",
        ])];
        let table = render(&columns, &rows, Some(34));
        assert_eq!(
            table.lines().next().unwrap(),
            "NAME    DEFINED  SECRET  USED IN"
        );
        assert!(table.contains("true     false"));
        assert!(table.lines().all(|line| line.width() <= 34));
        assert!(render(&columns, &rows, Some(20)).contains("true     false"));
        assert_eq!(render(&columns, &rows, Some(1)), "…\n…\n");
        assert_eq!(render(&columns, &rows, Some(4)), "D…S…\nt…f…\n");
        assert_eq!(render(&columns, &rows, Some(7)), " D… S…\n t… f…\n");
        for width in 0..34 {
            let table = render(&columns, &rows, Some(width));
            assert_eq!(table.lines().count(), 2, "width {width}");
            if width > 0 {
                assert!(
                    table.lines().all(|line| !line.trim().is_empty()),
                    "width {width}"
                );
            }
            assert!(
                table.lines().all(|line| line.width() <= width),
                "width {width}: {table:?}"
            );
        }
    }

    #[test]
    fn fallback_is_deterministic_and_bounds_outliers() {
        let columns = [Column::text("NAME", 32), Column::remaining("EXTENDS")];
        let rows = [["n".repeat(200), "p".repeat(300)]];
        let table = render(&columns, &rows, None);
        assert_eq!(table, render(&columns, &rows, Some(120)));
        assert!(table.lines().all(|line| line.width() <= 120));
        assert!(table.lines().nth(1).unwrap().ends_with('…'));
    }

    #[test]
    fn controls_cannot_create_rows_or_tab_stops() {
        let columns = [Column::text("NAME", 32), Column::remaining("URL")];
        let table = render(
            &columns,
            &[row(["one\ntwo\tthree\r", "\u{1b}[31m/value"])],
            Some(80),
        );
        assert_eq!(table.lines().count(), 2);
        assert!(!table.contains(['\t', '\r', '\u{1b}']));
        assert!(table.contains("one two three"));
    }

    #[test]
    fn empty_tables_keep_headers_and_zero_columns_are_empty() {
        assert_eq!(
            render(
                &[Column::text("NAME", 32), Column::remaining("EXTENDS")],
                &[],
                None
            ),
            "NAME  EXTENDS\n"
        );
        assert_eq!(render(&[], &[[]], None), "");
    }
}
