//! One-line human tables. JSON never passes through this module.

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

fn terminal_width(is_terminal: bool, size: impl FnOnce() -> Option<usize>) -> Option<usize> {
    // Redirected output must be deterministic, even when stderr has a terminal.
    if !is_terminal {
        return None;
    }
    size()
}

pub(crate) fn render_terminal<const N: usize>(
    columns: &[Column; N],
    rows: &[[String; N]],
) -> String {
    let width = terminal_width(stdout().is_terminal(), || {
        terminal_size::terminal_size().map(|(terminal_size::Width(width), _)| usize::from(width))
    });
    render(columns, rows, width)
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
    // Strip terminal sequences before normalizing characters and measuring cells.
    let rows: Vec<[String; N]> = rows
        .iter()
        .map(|row| row.each_ref().map(|cell| sanitize(cell)))
        .collect();
    let mut widths: [usize; N] = std::array::from_fn(|index| {
        rows.iter()
            .map(|row| display_width(&row[index]))
            .chain(std::iter::once(display_width(columns[index].header)))
            .max()
            .unwrap_or(0)
            .min(columns[index].maximum)
            .min(budget)
    });
    let compact_floors: [usize; N] = std::array::from_fn(|index| {
        if columns[index].compact {
            widths[index]
        } else {
            0
        }
    });
    let header_floors: [usize; N] = std::array::from_fn(|index| {
        compact_floors[index].max(display_width(columns[index].header).min(widths[index]))
    });
    // Preserve compact data and flexible headers, then compact data alone, then
    // allow every column to shrink. Fixed floors make those priorities explicit.
    for floors in [header_floors, compact_floors, [0; N]] {
        for _ in 0..widths.iter().sum::<usize>().saturating_sub(budget) {
            let candidate = (0..N)
                .filter(|&index| widths[index] > floors[index])
                .max_by_key(|&index| widths[index] - floors[index]);
            let Some(index) = candidate else { break };
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
            line.push_str(&" ".repeat(widths[index] - display_width(&cell) + gap));
        }
        output.push_str(line.trim_end());
        output.push('\n');
    }
    output
}

fn sanitize(value: &str) -> String {
    let mut text = Sanitized(String::new());
    vte::Parser::new().advance(&mut text, value.as_bytes());
    text.0
}

struct Sanitized(String);

// Keep printable text and normalize controls. Default callbacks discard entire
// CSI/OSC sequences, including OSC payloads and their terminators.
impl vte::Perform for Sanitized {
    fn print(&mut self, ch: char) {
        self.0.push(
            if ch.is_control()
                || matches!(ch,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}'
            | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                ' '
            } else {
                ch
            },
        );
    }

    fn execute(&mut self, _byte: u8) {
        self.0.push(' ');
    }
}

// Preserve unicode-width's cross-grapheme shaping rules in every layout check.
fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

fn truncate(value: &str, width: usize, policy: Truncation) -> String {
    if display_width(value) <= width {
        return value.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    // Reserve roughly half the text space for the suffix in middle truncation.
    // The prefix limit includes the ellipsis; measure complete candidates so
    // ligatures across grapheme boundaries retain their whole-string width.
    let prefix_limit = match policy {
        Truncation::End => width,
        Truncation::Middle => width / 2 + 1,
    };
    let mut prefix = String::new();
    for grapheme in value.graphemes(true) {
        let candidate = format!("{prefix}{grapheme}…");
        if display_width(&candidate) > prefix_limit {
            break;
        }
        prefix.push_str(grapheme);
    }
    let mut output = format!("{prefix}…");
    if matches!(policy, Truncation::Middle) {
        let mut suffix = String::new();
        for grapheme in value[prefix.len()..].graphemes(true).rev() {
            let candidate = format!("{prefix}…{grapheme}{suffix}");
            if display_width(&candidate) > width {
                break;
            }
            suffix.insert_str(0, grapheme);
            output = candidate;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row<const N: usize>(cells: [&str; N]) -> [String; N] {
        cells.map(str::to_owned)
    }

    #[test]
    fn tty_uses_available_terminal_size() {
        assert_eq!(terminal_width(true, || Some(42)), Some(42));
    }

    #[test]
    fn redirected_stdout_never_queries_terminal_size() {
        assert_eq!(
            terminal_width(false, || panic!("queried terminal size for piped output")),
            None
        );
    }

    #[test]
    fn tty_without_terminal_size_uses_fallback() {
        let width = terminal_width(true, || None);
        assert_eq!(width, None);
        let columns = [Column::remaining("VALUE")];
        let rows = [row([&"x".repeat(121)])];
        assert_eq!(
            render(&columns, &rows, width),
            format!("VALUE\n{}…\n", "x".repeat(119))
        );
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
    fn flag_and_skin_tone_emoji_stay_whole_and_align() {
        let columns = [Column::text("NAME", 32), Column::remaining("URL")];
        let rows = [
            row(["🇦🇺", "/flag"]),
            row(["👍🏽", "/skin"]),
            row(["👩🏽‍💻", "/work"]),
        ];
        assert_eq!(
            render(&columns, &rows, Some(80)),
            "NAME  URL\n🇦🇺    /flag\n👍🏽    /skin\n👩🏽‍💻    /work\n"
        );
        for emoji in ["🇦🇺", "👍🏽", "👩🏽‍💻"] {
            assert_eq!(
                truncate(&format!("{emoji}abcd"), 3, Truncation::End),
                format!("{emoji}…")
            );
            assert_eq!(
                truncate(&format!("ab{emoji}"), 5, Truncation::End),
                format!("ab{emoji}")
            );
            assert_eq!(
                truncate(&format!("abcd{emoji}"), 5, Truncation::Middle),
                format!("ab…{emoji}")
            );
        }
    }

    #[test]
    fn fit_and_clipping_use_whole_string_width() {
        // These separate graphemes contract to one cell in whole-string width.
        let lam_alef = "لا";
        assert_eq!(lam_alef.width(), 1);
        assert_eq!(display_width(lam_alef), 1);
        assert_eq!(truncate(lam_alef, 1, Truncation::End), lam_alef);
        assert_eq!(truncate(lam_alef, 2, Truncation::End), lam_alef);
        assert_eq!(
            truncate(&format!("{lam_alef}abcd"), 3, Truncation::End),
            "لاa…"
        );
        assert_eq!(truncate("abcdلا", 3, Truncation::Middle), "a…لا");
    }

    #[test]
    fn lam_alef_measurement_fit_and_padding_agree() {
        let columns = [Column::remaining("X"), Column::remaining("Y")];
        let rows = [row(["لا", "L"]), row(["a", "A"])];
        assert_eq!(render(&columns, &rows, Some(5)), "X  Y\nلا  L\na  A\n");
        assert_eq!(
            render(&[Column::remaining("X")], &[row(["لا"])], Some(2)),
            "X\nلا\n"
        );
        assert_eq!(render(&[Column::remaining("لا")], &[], Some(2)), "لا\n");
        assert_eq!(
            render(&[Column::remaining("X")], &[row(["لا"])], Some(1)),
            "X\nلا\n"
        );
    }

    #[test]
    fn shaped_text_stays_within_column_and_row_bounds() {
        let values = [
            "لا".repeat(40),
            "لاabcd".repeat(10),
            "abcdلا".repeat(10),
            "🇦🇺👍🏽لاe\u{301}".repeat(10),
        ];
        for width in 0..34 {
            for value in &values {
                for policy in [Truncation::End, Truncation::Middle] {
                    let cell = truncate(value, width, policy);
                    assert!(display_width(&cell) <= width, "width {width}: {cell:?}");
                }
                let columns = [Column::selector("SELECTOR", 28), Column::remaining("VALUE")];
                let table = render(&columns, &[[value.clone(), value.clone()]], Some(width));
                assert!(
                    table.lines().all(|line| line.width() <= width),
                    "width {width}: {table:?}"
                );
            }
        }
    }

    #[test]
    fn bidi_controls_and_unicode_separators_become_spaces() {
        for ch in [
            '\u{061c}', '\u{200e}', '\u{200f}', '\u{2028}', '\u{2029}', '\u{202a}', '\u{202b}',
            '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
        ] {
            assert_eq!(sanitize(&format!("a{ch}b")), "a b", "U+{:04X}", ch as u32);
        }
        assert_eq!(
            render(
                &[Column::text("NAME", 32)],
                &[row(["left\u{202e}right\u{2028}next"])],
                None
            ),
            "NAME\nleft right next\n"
        );
    }

    #[test]
    fn entire_csi_and_osc_sequences_are_removed() {
        for (input, expected) in [
            ("\u{1b}[31mred\u{1b}[0m", "red"),
            ("a\u{1b}[2Jb", "ab"),
            ("\u{1b}]0;window title\u{7}visible", "visible"),
            (
                "\u{1b}]8;;https://example.com\u{1b}\\link\u{1b}]8;;\u{1b}\\",
                "link",
            ),
            ("start\u{1b}[31", "start"),
            ("start\u{1b}]0;unfinished", "start"),
            ("literal [31m and ]0;title", "literal [31m and ]0;title"),
        ] {
            assert_eq!(sanitize(input), expected);
        }
    }

    #[test]
    fn standalone_ascii_and_c1_controls_become_spaces() {
        for ch in [
            '\0', '\u{7}', '\t', '\n', '\r', '\u{7f}', '\u{85}', '\u{9f}',
        ] {
            assert_eq!(sanitize(&format!("a{ch}b")), "a b");
        }
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
        assert!(table.contains("/value"));
        assert!(!table.contains("[31m"));
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
