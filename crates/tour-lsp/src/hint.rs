use lsp_types::{InlayHint, InlayHintLabel, Position, Range};

use crate::{settings::Placement, tour::TourStep};

/// Longest body hint that is rendered. Additional wrapped lines are dropped.
pub const MAX_BODY_LINES: usize = 6;
/// Longest body line, excluding the leading padding space.
pub const MAX_BODY_WIDTH: usize = 80;

const TRUNCATION_MARKER: char = '…';

/// Build the single body inlay hint for a step.
///
/// The hint has no kind and sets `paddingLeft`, which gives it a right-biased
/// anchor in Zed. Every line starts with one padding space, so a code lens
/// anchored at the same line stays above the body.
pub fn body_hint(
    step: &TourStep,
    source_range: Range,
    source: &str,
    placement: Placement,
) -> Option<InlayHint> {
    let text = format_body(step.body_hint())?;
    let (position, label) = match placement {
        Placement::Above => (Position::new(source_range.start.line, 0), text),
        Placement::Below => {
            let next_line = source_range.end.line.saturating_add(1);
            if next_line < source_line_count(source) {
                (Position::new(next_line, 0), text)
            } else {
                let line_end = source_line(source, source_range.end.line)
                    .map(utf16_len)
                    .unwrap_or(source_range.end.character);
                (
                    Position::new(source_range.end.line, line_end),
                    format!("\n{text}"),
                )
            }
        }
    };

    Some(InlayHint {
        position,
        label: InlayHintLabel::String(label),
        kind: None,
        text_edits: None,
        tooltip: None,
        padding_left: Some(true),
        padding_right: None,
        data: None,
    })
}

/// Format the plain-text body as padded hint lines that end with a newline.
///
/// Returns `None` for an empty body. Author line breaks are kept for lines that
/// fit. Longer lines wrap at word boundaries to `MAX_BODY_WIDTH` body columns,
/// and a word longer than that budget is hard-wrapped. Wrapped lines beyond
/// `MAX_BODY_LINES` are dropped and the last retained line gets a trailing
/// ellipsis. The limits keep a long step from covering the viewport and are
/// documented in `docs/body-hint.md`.
pub fn format_body(body: &str) -> Option<String> {
    let lines: Vec<String> = body
        .lines()
        .map(str::trim_end)
        .flat_map(wrap_line)
        .collect();

    let first = lines.iter().position(|line| !line.is_empty())?;
    let last = lines.iter().rposition(|line| !line.is_empty())?;
    let mut lines = lines[first..=last].to_vec();

    if lines.len() > MAX_BODY_LINES {
        lines.truncate(MAX_BODY_LINES);
        if let Some(last) = lines.last_mut() {
            let budget = MAX_BODY_WIDTH.saturating_sub(2);
            if last.chars().count() > budget {
                *last = last.chars().take(budget).collect();
            }
            last.push(' ');
            last.push(TRUNCATION_MARKER);
        }
    }

    let width = lines.iter().map(|line| line.chars().count()).max()?;
    let mut text = String::with_capacity((width + 2) * lines.len());
    for line in lines {
        text.push(' ');
        text.push_str(&line);
        text.extend(std::iter::repeat_n(
            ' ',
            width.saturating_sub(line.chars().count()),
        ));
        text.push('\n');
    }

    Some(text)
}

fn wrap_line(line: &str) -> Vec<String> {
    if line.chars().count() <= MAX_BODY_WIDTH {
        return vec![line.to_owned()];
    }

    let mut wrapped = Vec::new();
    let mut remaining = line;
    let mut first = true;

    while !remaining.is_empty() {
        if !first {
            remaining = remaining.trim_start();
        }
        if remaining.is_empty() {
            break;
        }
        if remaining.chars().count() <= MAX_BODY_WIDTH {
            wrapped.push(remaining.to_owned());
            break;
        }

        let limit = remaining
            .char_indices()
            .nth(MAX_BODY_WIDTH)
            .map_or(remaining.len(), |(index, _)| index);
        let head = &remaining[..limit];

        if let Some((index, _)) = head.char_indices().rev().find(|(_, ch)| ch.is_whitespace()) {
            let before = remaining[..index].trim_end();
            if !before.is_empty() {
                wrapped.push(before.to_owned());
                remaining = &remaining[index..];
                first = false;
                continue;
            }
        }

        let hard = remaining[..limit].trim_end();
        if !hard.is_empty() {
            wrapped.push(hard.to_owned());
        }
        remaining = &remaining[limit..];
        first = false;
    }

    if wrapped.is_empty() {
        wrapped.push(String::new());
    }

    wrapped
}

pub(crate) fn source_line_count(source: &str) -> u32 {
    u32::try_from(source.split('\n').count()).unwrap_or(u32::MAX)
}

/// The LSP position at the end of the document text.
///
/// A trailing newline creates an empty final line, so the end position is the
/// start of that line. The character offset uses UTF-16 code units, like every
/// other position the server derives from source text.
pub(crate) fn source_end_position(source: &str) -> Position {
    let last_line = source_line_count(source).saturating_sub(1);
    let character = source_line(source, last_line).map(utf16_len).unwrap_or(0);
    Position::new(last_line, character)
}

fn source_line(source: &str, line: u32) -> Option<&str> {
    let line = usize::try_from(line).ok()?;
    source
        .split('\n')
        .nth(line)
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

fn utf16_len(text: &str) -> u32 {
    u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use lsp_types::{InlayHint, InlayHintLabel, Position, Range};

    use super::{MAX_BODY_LINES, MAX_BODY_WIDTH, body_hint, format_body, source_end_position};
    use crate::{settings::Placement, tour::parse_tour};

    fn step(hint: &str) -> crate::tour::TourStep {
        let source = format!(
            r#"# Hint

## 1. Entry
[target](src/main.rs#L1:1-L1:2)

{hint}
"#
        );
        parse_tour("/tours/example.tour.md", &source)
            .expect("tour should parse")
            .steps
            .remove(0)
    }

    fn range(start_line: u32, end_line: u32) -> Range {
        Range::new(Position::new(start_line, 0), Position::new(end_line, 4))
    }

    fn hint_label(hint: &InlayHint) -> &str {
        match &hint.label {
            InlayHintLabel::String(label) => label,
            InlayHintLabel::LabelParts(_) => panic!("expected a string label"),
        }
    }

    #[test]
    fn formats_padded_lines_without_a_left_rule_and_with_a_trailing_newline() {
        let text = format_body("First line.\nSecond.").expect("body should format");

        assert_eq!(text, " First line.\n Second.    \n");
        assert!(text.ends_with('\n'));
        assert!(!text.contains('▎'));

        let lines: Vec<_> = text.split_terminator('\n').collect();
        assert!(lines.iter().all(|line| line.starts_with(' ')));
        assert!(lines.iter().all(|line| !line.starts_with("  ")));
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0].chars().count(),
            lines[1].chars().count(),
            "body lines should share a consistent width"
        );
    }

    #[test]
    fn skips_empty_bodies() {
        assert_eq!(format_body(""), None);
        assert_eq!(format_body(" \n\t\n"), None);
    }

    #[test]
    fn preserves_author_line_breaks_within_the_budget() {
        let text = format_body("short first\nshort second").expect("body should format");
        let lines: Vec<_> = text.lines().collect();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].trim_end(), " short first");
        assert_eq!(lines[1].trim_end(), " short second");
    }

    #[test]
    fn wraps_long_lines_at_word_boundaries_without_truncating() {
        let body = format!("{} tail", "word ".repeat(20).trim_end());
        let text = format_body(&body).expect("body should format");
        let lines: Vec<_> = text.lines().collect();

        assert_eq!(lines.len(), 2);
        assert!(!text.contains('…'), "wrapped lines are not truncated");
        assert!(
            lines
                .iter()
                .all(|line| line.trim_start().chars().count() <= MAX_BODY_WIDTH)
        );
        let body_lines: Vec<_> = lines
            .iter()
            .map(|line| line.trim_start().trim_end())
            .collect();
        assert_eq!(body_lines[0], "word ".repeat(16).trim_end());
        assert_eq!(body_lines[1], "word word word word tail");
    }

    #[test]
    fn hard_wraps_words_longer_than_the_budget() {
        let text = format_body(&"x".repeat(MAX_BODY_WIDTH * 2 + 5)).expect("body should format");
        let widths: Vec<_> = text
            .lines()
            .map(|line| line.trim_start().trim_end().chars().count())
            .collect();

        assert_eq!(widths, vec![MAX_BODY_WIDTH, MAX_BODY_WIDTH, 5]);
        assert!(!text.contains('…'), "hard wrapping is not truncation");
    }

    #[test]
    fn wrapped_lines_keep_author_indentation_on_the_first_line() {
        let body = format!("    {}", "word ".repeat(20).trim_end());
        let text = format_body(&body).expect("body should format");
        let lines: Vec<_> = text.lines().collect();

        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].starts_with("     word"),
            "first wrapped line should keep the author indentation"
        );
        assert!(
            lines[1].starts_with(" word"),
            "continuation lines should only have the hint padding"
        );
    }

    #[test]
    fn clamps_long_bodies_to_the_line_budget() {
        let body = (0..MAX_BODY_LINES + 2)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let text = format_body(&body).expect("body should format");

        assert_eq!(text.lines().count(), MAX_BODY_LINES);
        assert!(text.lines().last().unwrap().ends_with(" …"));
        assert_eq!(text.matches('…').count(), 1);
        assert!(
            text.lines()
                .all(|line| line.trim_start().chars().count() <= MAX_BODY_WIDTH)
        );

        let long_line = "word ".repeat(MAX_BODY_LINES * 20).trim_end().to_owned();
        let text = format_body(&long_line).expect("body should format");

        assert_eq!(text.lines().count(), MAX_BODY_LINES);
        assert!(text.lines().last().unwrap().ends_with(" …"));
        assert_eq!(text.matches('…').count(), 1);
        assert!(
            text.lines()
                .all(|line| line.trim_start().chars().count() <= MAX_BODY_WIDTH)
        );
    }

    #[test]
    fn below_placement_uses_the_next_line() {
        let hint = body_hint(
            &step("Body."),
            range(1, 1),
            "line0\nline1\nline2\n",
            Placement::Below,
        )
        .expect("hint should exist");

        assert_eq!(hint.position, Position::new(2, 0));
        assert_eq!(hint.kind, None);
        assert_eq!(hint.padding_left, Some(true));
        assert_eq!(hint_label(&hint), " Body.\n");
    }

    #[test]
    fn above_placement_uses_the_selection_start_line() {
        let hint = body_hint(
            &step("Body."),
            range(1, 1),
            "line0\nline1\nline2\n",
            Placement::Above,
        )
        .expect("hint should exist");

        assert_eq!(hint.position, Position::new(1, 0));
        assert_eq!(hint_label(&hint), " Body.\n");
    }

    #[test]
    fn last_line_fallback_without_a_trailing_newline() {
        let hint = body_hint(
            &step("Body."),
            range(1, 1),
            "line0\nline1",
            Placement::Below,
        )
        .expect("hint should exist");

        assert_eq!(hint.position, Position::new(1, 5));
        assert_eq!(hint_label(&hint), "\n Body.\n");
        assert_eq!(hint.padding_left, Some(true));
    }

    #[test]
    fn trailing_newline_creates_an_empty_line_after_the_selection() {
        let hint = body_hint(
            &step("Body."),
            range(1, 1),
            "line0\nline1\n",
            Placement::Below,
        )
        .expect("hint should exist");

        assert_eq!(hint.position, Position::new(2, 0));
        assert_eq!(hint_label(&hint), " Body.\n");
    }

    #[test]
    fn last_line_fallback_uses_utf16_line_length() {
        let hint = body_hint(&step("Body."), range(0, 0), "a😀", Placement::Below)
            .expect("hint should exist");

        assert_eq!(hint.position, Position::new(0, 3));
        assert!(hint_label(&hint).starts_with('\n'));
    }

    #[test]
    fn end_position_handles_newlines_crlf_and_utf16() {
        assert_eq!(source_end_position(""), Position::new(0, 0));
        assert_eq!(source_end_position("abc"), Position::new(0, 3));
        assert_eq!(source_end_position("abc\n"), Position::new(1, 0));
        assert_eq!(source_end_position("abc\ndef"), Position::new(1, 3));
        assert_eq!(source_end_position("abc\ndef\r\n"), Position::new(2, 0));
        assert_eq!(source_end_position("a😀"), Position::new(0, 3));
    }
}
