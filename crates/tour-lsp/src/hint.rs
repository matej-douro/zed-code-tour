use lsp_types::{InlayHint, InlayHintLabel, Position, Range};

use crate::{settings::Placement, tour::TourStep};

/// The left rule used by every body line.
pub const LEFT_RULE: char = '▎';
/// Longest body hint that is rendered. Additional lines are dropped.
pub const MAX_BODY_LINES: usize = 6;
/// Longest body line, excluding the padding and the left rule.
pub const MAX_BODY_WIDTH: usize = 80;

const TRUNCATION_MARKER: char = '…';

/// Build the single body inlay hint for a step.
///
/// The hint has no kind and sets `paddingLeft`, which gives it a right-biased
/// anchor in Zed. Every line starts with one padding space before the left
/// rule, so a code lens anchored at the same line stays above the body.
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
/// Returns `None` for an empty body. The line and width limits keep a long step
/// from covering the viewport. Truncation uses an ellipsis and is documented in
/// `docs/body-hint.md`.
pub fn format_body(body: &str) -> Option<String> {
    let mut lines: Vec<String> = body.lines().map(str::trim_end).map(str::to_owned).collect();

    while lines.first().is_some_and(|line| line.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return None;
    }

    if lines.len() > MAX_BODY_LINES {
        lines.truncate(MAX_BODY_LINES);
        if let Some(last) = lines.last_mut() {
            last.push(' ');
            last.push(TRUNCATION_MARKER);
        }
    }

    for line in &mut lines {
        if line.chars().count() > MAX_BODY_WIDTH {
            *line = truncate_chars(line, MAX_BODY_WIDTH);
        }
    }

    let width = lines.iter().map(|line| line.chars().count()).max()?;
    let mut text = String::with_capacity((width + 4) * lines.len());
    for line in lines {
        text.push(' ');
        text.push(LEFT_RULE);
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

fn truncate_chars(line: &str, width: usize) -> String {
    if line.chars().count() <= width {
        return line.to_owned();
    }

    let mut truncated: String = line.chars().take(width.saturating_sub(1)).collect();
    truncated.push(TRUNCATION_MARKER);
    truncated
}

pub(crate) fn source_line_count(source: &str) -> u32 {
    u32::try_from(source.split('\n').count()).unwrap_or(u32::MAX)
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

    use super::{MAX_BODY_LINES, MAX_BODY_WIDTH, body_hint, format_body};
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
    fn formats_padded_lines_with_the_left_rule_and_trailing_newline() {
        let text = format_body("First line.\nSecond.").expect("body should format");

        assert_eq!(text, " ▎ First line.\n ▎ Second.    \n");
        assert!(text.ends_with('\n'));

        let lines: Vec<_> = text.split_terminator('\n').collect();
        assert!(lines.iter().all(|line| line.starts_with(" ▎ ")));
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
    fn clamps_long_bodies() {
        let body = (0..MAX_BODY_LINES + 2)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let text = format_body(&body).expect("body should format");

        assert_eq!(text.lines().count(), MAX_BODY_LINES);
        assert!(text.lines().last().unwrap().ends_with(" …"));

        let long_line = "x".repeat(MAX_BODY_WIDTH + 20);
        let text = format_body(&long_line).expect("body should format");
        let body_line = text.trim_start_matches(" ▎ ").trim_end();
        assert_eq!(body_line.chars().count(), MAX_BODY_WIDTH);
        assert!(body_line.ends_with('…'));
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
        assert_eq!(hint_label(&hint), " ▎ Body.\n");
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
        assert_eq!(hint_label(&hint), " ▎ Body.\n");
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
        assert_eq!(hint_label(&hint), "\n ▎ Body.\n");
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
        assert_eq!(hint_label(&hint), " ▎ Body.\n");
    }

    #[test]
    fn last_line_fallback_uses_utf16_line_length() {
        let hint = body_hint(&step("Body."), range(0, 0), "a😀", Placement::Below)
            .expect("hint should exist");

        assert_eq!(hint.position, Position::new(0, 3));
        assert!(hint_label(&hint).starts_with('\n'));
    }
}
