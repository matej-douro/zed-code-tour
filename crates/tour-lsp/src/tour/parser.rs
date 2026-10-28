use std::{
    fmt,
    path::{Path, PathBuf},
};

use serde_json::Value;

use super::model::{ThreadLink, Tour, TourPosition, TourRange, TourStep, TourTarget};

const THREAD_URI_PREFIX: &str = "zed:///agent/thread/";
const TOUR_STEP_METADATA_PREFIX: &str = "tour-step:";

/// Errors returned when a tour document cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line in the tour file, when the error has a source location.
    pub line: Option<u32>,
    pub kind: ParseErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    MissingTitle,
    MalformedTitle,
    MalformedStepHeading,
    MissingStepLink,
    MalformedLink,
    UnsupportedTarget,
    MissingRangeFragment,
    MalformedRange,
    InvalidRange,
    MalformedHintMetadata,
}

impl ParseError {
    fn new(line: Option<u32>, kind: ParseErrorKind, message: impl Into<String>) -> Self {
        Self {
            line,
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse a `.tour.md` document.
///
/// Relative target paths are resolved against the tour file's directory. The
/// parser returns every validation error it can find instead of stopping at the
/// first one, and never panics on malformed input.
pub fn parse_tour(tour_path: impl AsRef<Path>, source: &str) -> Result<Tour, Vec<ParseError>> {
    let tour_path = tour_path.as_ref();
    let base_dir = tour_path.parent().unwrap_or_else(|| Path::new(""));

    let mut errors = Vec::new();
    let mut title: Option<String> = None;
    let mut thread_link: Option<ThreadLink> = None;
    let mut steps = Vec::new();
    let mut current_step: Option<StepBuilder> = None;

    for (index, raw_line) in source.lines().enumerate() {
        let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let trimmed = line.trim_start();

        if let Some(rest) = trimmed.strip_prefix("## ") {
            finish_step(current_step.take(), base_dir, &mut errors, &mut steps);

            match parse_step_heading(rest) {
                Ok((number, step_title)) => {
                    current_step = Some(StepBuilder {
                        number,
                        title: step_title,
                        heading_line: line_number,
                        lines: Vec::new(),
                    });
                }
                Err(message) => errors.push(ParseError::new(
                    Some(line_number),
                    ParseErrorKind::MalformedStepHeading,
                    message,
                )),
            }
            continue;
        }

        if trimmed.starts_with("##") && !trimmed.starts_with("###") {
            finish_step(current_step.take(), base_dir, &mut errors, &mut steps);
            errors.push(ParseError::new(
                Some(line_number),
                ParseErrorKind::MalformedStepHeading,
                format!("malformed step heading `{}`", trimmed.trim_end()),
            ));
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("# ") {
            if let Some(step) = current_step.as_mut() {
                step.lines.push((line_number, line.to_owned()));
            } else if title.is_none() {
                let parsed_title = rest.trim();
                if parsed_title.is_empty() {
                    errors.push(ParseError::new(
                        Some(line_number),
                        ParseErrorKind::MalformedTitle,
                        "tour title must not be empty",
                    ));
                } else {
                    title = Some(parsed_title.to_owned());
                }
            }
            continue;
        }

        if let Some(step) = current_step.as_mut() {
            step.lines.push((line_number, line.to_owned()));
        } else if thread_link.is_none()
            && let Some(link) = find_first_markdown_link(line)
            && link.destination.starts_with(THREAD_URI_PREFIX)
        {
            thread_link = Some(ThreadLink {
                text: link.text,
                uri: link.destination,
            });
        }
    }

    finish_step(current_step.take(), base_dir, &mut errors, &mut steps);

    if title.is_none() {
        errors.push(ParseError::new(
            None,
            ParseErrorKind::MissingTitle,
            "tour is missing a `# Title` heading",
        ));
    }

    if errors.is_empty() {
        Ok(Tour {
            title: title.unwrap_or_default(),
            thread_link,
            steps,
        })
    } else {
        errors.sort_by_key(|error| error.line.unwrap_or(0));
        Err(errors)
    }
}

struct StepBuilder {
    number: u32,
    title: String,
    heading_line: u32,
    lines: Vec<(u32, String)>,
}

fn finish_step(
    builder: Option<StepBuilder>,
    base_dir: &Path,
    errors: &mut Vec<ParseError>,
    steps: &mut Vec<TourStep>,
) {
    if let Some(builder) = builder
        && let Some(step) = parse_step(builder, base_dir, errors)
    {
        steps.push(step);
    }
}

fn parse_step(
    builder: StepBuilder,
    base_dir: &Path,
    errors: &mut Vec<ParseError>,
) -> Option<TourStep> {
    let mut target_link: Option<(u32, MarkdownLink)> = None;
    let mut hidden_spans = Vec::with_capacity(builder.lines.len());
    let mut hint = None;

    for (line_number, line) in &builder.lines {
        let comment_spans = html_comment_spans(line);
        if target_link.is_none() {
            let visible_line = mask_spans(line, &comment_spans);
            if let Some(link) = find_first_markdown_link(&visible_line) {
                target_link = Some((*line_number, link));
            }
        }

        match parse_step_metadata(line) {
            Ok(Some(comment_hint)) => {
                if hint.is_none() {
                    hint = Some(comment_hint);
                }
            }
            Ok(None) => {}
            Err(message) => errors.push(ParseError::new(
                Some(*line_number),
                ParseErrorKind::MalformedHintMetadata,
                message,
            )),
        }

        hidden_spans.push((*line_number, comment_spans));
    }

    let (target_line, target_link) = match target_link {
        Some(target) => target,
        None => {
            errors.push(ParseError::new(
                Some(builder.heading_line),
                ParseErrorKind::MissingStepLink,
                format!(
                    "step {} `{}` has no target link",
                    builder.number, builder.title
                ),
            ));
            return None;
        }
    };

    let target = match parse_target(&target_link, target_line, base_dir) {
        Ok(target) => target,
        Err(error) => {
            errors.push(error);
            return None;
        }
    };

    let mut body_lines = Vec::with_capacity(builder.lines.len());
    for ((line_number, line), (_, comment_spans)) in builder.lines.iter().zip(hidden_spans.iter()) {
        let mut spans = comment_spans.clone();
        if *line_number == target_line {
            spans.push((target_link.start, target_link.end));
        }

        spans.sort_unstable_by_key(|span| std::cmp::Reverse(span.0));
        let mut cleaned = line.clone();
        for (start, end) in spans {
            if start < end && end <= cleaned.len() {
                cleaned.replace_range(start..end, "");
            }
        }
        body_lines.push(cleaned);
    }

    Some(TourStep {
        number: builder.number,
        title: builder.title,
        target,
        body: body_lines.join("\n").trim().to_owned(),
        hint,
    })
}

fn parse_step_heading(rest: &str) -> Result<(u32, String), String> {
    let rest = rest.trim();
    let digit_end = rest
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(rest.len());

    if digit_end == 0 {
        return Err(format!("malformed step heading `## {rest}`"));
    }

    let number = rest[..digit_end]
        .parse::<u32>()
        .map_err(|_| format!("invalid step number in `## {rest}`"))?;
    let after_number = rest[digit_end..].trim_start();
    let title = after_number
        .strip_prefix('.')
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .ok_or_else(|| format!("malformed step heading `## {rest}`"))?;

    Ok((number, title.to_owned()))
}

fn parse_target(link: &MarkdownLink, line: u32, base_dir: &Path) -> Result<TourTarget, ParseError> {
    let destination = link.destination.trim();
    if destination.is_empty() {
        return Err(ParseError::new(
            Some(line),
            ParseErrorKind::MalformedLink,
            "target link has an empty destination",
        ));
    }

    let (raw_path, fragment) = destination.split_once('#').ok_or_else(|| {
        ParseError::new(
            Some(line),
            ParseErrorKind::MissingRangeFragment,
            format!(
                "target link `{}` is missing a #L<line>:<column>-L<line>:<column> fragment",
                destination
            ),
        )
    })?;

    if raw_path.is_empty() {
        return Err(ParseError::new(
            Some(line),
            ParseErrorKind::UnsupportedTarget,
            format!("target link `{destination}` has no file path"),
        ));
    }

    if raw_path.contains("://") {
        return Err(ParseError::new(
            Some(line),
            ParseErrorKind::UnsupportedTarget,
            format!("target link `{destination}` is not a file path"),
        ));
    }

    let range = parse_range(fragment, Some(line))?;
    let path = if Path::new(raw_path).is_absolute() {
        PathBuf::from(raw_path)
    } else {
        base_dir.join(raw_path)
    };

    Ok(TourTarget {
        link_text: link.text.clone(),
        raw_path: raw_path.to_owned(),
        path,
        range,
    })
}

fn parse_range(fragment: &str, line: Option<u32>) -> Result<TourRange, ParseError> {
    let rest = fragment
        .strip_prefix('L')
        .ok_or_else(|| malformed_range(line, fragment, "fragment must start with `L`"))?;
    let (start, end) = rest
        .split_once("-L")
        .ok_or_else(|| malformed_range(line, fragment, "fragment must contain `-L`"))?;
    let start = parse_position(start, line, fragment)?;
    let end = parse_position(end, line, fragment)?;

    if start.line == 0 || start.column == 0 || end.line == 0 || end.column == 0 {
        return Err(ParseError::new(
            line,
            ParseErrorKind::InvalidRange,
            format!("range `#{fragment}` uses 1-based lines and columns, so zero is not allowed"),
        ));
    }

    if start.line > end.line || (start.line == end.line && start.column > end.column) {
        return Err(ParseError::new(
            line,
            ParseErrorKind::InvalidRange,
            format!("range `#{fragment}` ends before it starts"),
        ));
    }

    Ok(TourRange { start, end })
}

fn parse_position(
    position: &str,
    line: Option<u32>,
    fragment: &str,
) -> Result<TourPosition, ParseError> {
    let (line_number, column) = position
        .split_once(':')
        .ok_or_else(|| malformed_range(line, fragment, "positions must use `<line>:<column>`"))?;
    let line_number = line_number.parse::<u32>().map_err(|_| {
        malformed_range(
            line,
            fragment,
            "line number is not a valid positive integer",
        )
    })?;
    let column = column.parse::<u32>().map_err(|_| {
        malformed_range(
            line,
            fragment,
            "column number is not a valid positive integer",
        )
    })?;

    Ok(TourPosition {
        line: line_number,
        column,
    })
}

fn malformed_range(line: Option<u32>, fragment: &str, detail: &str) -> ParseError {
    ParseError::new(
        line,
        ParseErrorKind::MalformedRange,
        format!("malformed range `#{fragment}`: {detail}"),
    )
}

struct MarkdownLink {
    text: String,
    destination: String,
    start: usize,
    end: usize,
}

fn find_first_markdown_link(text: &str) -> Option<MarkdownLink> {
    text.char_indices()
        .find_map(|(index, _)| markdown_link_at(text, index))
}

fn markdown_link_at(text: &str, index: usize) -> Option<MarkdownLink> {
    let bytes = text.as_bytes();
    if bytes.get(index) != Some(&b'[') || (index > 0 && bytes[index - 1] == b'!') {
        return None;
    }

    let close_bracket = find_byte(bytes, index + 1, b']')?;
    if bytes.get(close_bracket + 1) != Some(&b'(') {
        return None;
    }
    let close_paren = find_byte(bytes, close_bracket + 2, b')')?;
    let destination = normalize_destination(&text[close_bracket + 2..close_paren]);

    Some(MarkdownLink {
        text: text[index + 1..close_bracket].to_owned(),
        destination,
        start: index,
        end: close_paren + 1,
    })
}

fn find_byte(bytes: &[u8], start: usize, needle: u8) -> Option<usize> {
    bytes
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, byte)| (*byte == needle).then_some(index))
}

fn normalize_destination(destination: &str) -> String {
    let destination = destination.trim();
    if let Some(inner) = destination
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
    {
        inner.to_owned()
    } else {
        destination
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned()
    }
}

fn parse_step_metadata(line: &str) -> Result<Option<String>, String> {
    let mut search_start = 0;

    while let Some(offset) = line[search_start..].find("<!--") {
        let comment_start = search_start + offset;
        let content_start = comment_start + 4;
        let Some(close_offset) = line[content_start..].find("-->") else {
            if line[content_start..]
                .trim_start()
                .starts_with(TOUR_STEP_METADATA_PREFIX)
            {
                return Err(format!(
                    "unterminated `{TOUR_STEP_METADATA_PREFIX}` metadata comment"
                ));
            }
            return Ok(None);
        };
        let content_end = content_start + close_offset;
        let content = line[content_start..content_end].trim_start();

        if let Some(data) = content.strip_prefix(TOUR_STEP_METADATA_PREFIX) {
            let value: Value = serde_json::from_str(data.trim())
                .map_err(|error| format!("invalid tour-step metadata JSON: {error}"))?;
            let object = value
                .as_object()
                .ok_or_else(|| "tour-step metadata must be a JSON object".to_owned())?;

            return match object.get("hint") {
                None => Ok(None),
                Some(Value::String(hint)) => Ok(Some(hint.clone())),
                Some(_) => Err("tour-step metadata `hint` must be a string".to_owned()),
            };
        }

        search_start = content_end + 3;
    }

    Ok(None)
}

fn html_comment_spans(line: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut search_start = 0;

    while let Some(offset) = line[search_start..].find("<!--") {
        let comment_start = search_start + offset;
        let content_start = comment_start + 4;
        let Some(close_offset) = line[content_start..].find("-->") else {
            spans.push((comment_start, line.len()));
            break;
        };
        let comment_end = content_start + close_offset + 3;
        spans.push((comment_start, comment_end));
        search_start = comment_end;
    }

    spans
}

fn mask_spans(line: &str, spans: &[(usize, usize)]) -> String {
    let mut masked = line.to_owned();
    for &(start, end) in spans.iter().rev() {
        if start < end && end <= masked.len() {
            masked.replace_range(start..end, &" ".repeat(end - start));
        }
    }
    masked
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use lsp_types::{Position, Range};

    use super::{ParseError, ParseErrorKind, parse_tour};
    use crate::tour::{ThreadLink, Tour, TourPosition, TourRange};

    const TOUR_PATH: &str = "/tours/example.tour.md";

    fn parse(source: &str) -> Result<Tour, Vec<ParseError>> {
        parse_tour(Path::new(TOUR_PATH), source)
    }

    fn errors(source: &str) -> Vec<ParseError> {
        parse(source).expect_err("tour should not parse")
    }

    fn error_kinds(source: &str) -> Vec<ParseErrorKind> {
        errors(source).into_iter().map(|error| error.kind).collect()
    }

    #[test]
    fn parses_minimal_tour_with_title_thread_link_and_step() {
        let tour = parse(
            r#"# Auth flow

[Back to the thread](zed:///agent/thread/session-1?name=Auth)

## 1. Entry point
[src/main.rs:12:4-12:20](src/main.rs#L12:4-L12:20)

The program starts here, then calls the router at line 14.
"#,
        )
        .expect("tour should parse");

        assert_eq!(tour.title, "Auth flow");
        assert_eq!(
            tour.thread_link,
            Some(ThreadLink {
                text: "Back to the thread".to_owned(),
                uri: "zed:///agent/thread/session-1?name=Auth".to_owned(),
            })
        );
        assert_eq!(tour.steps.len(), 1);

        let step = &tour.steps[0];
        assert_eq!(step.number, 1);
        assert_eq!(step.title, "Entry point");
        assert_eq!(step.target.link_text, "src/main.rs:12:4-12:20");
        assert_eq!(step.target.raw_path, "src/main.rs");
        assert_eq!(step.target.path, Path::new("/tours/src/main.rs"));
        assert_eq!(
            step.target.range,
            TourRange {
                start: TourPosition {
                    line: 12,
                    column: 4
                },
                end: TourPosition {
                    line: 12,
                    column: 20
                },
            }
        );
        assert_eq!(
            step.body,
            "The program starts here, then calls the router at line 14."
        );
        assert_eq!(step.hint, None);
        assert_eq!(
            step.body_hint(),
            "The program starts here, then calls the router at line 14."
        );
    }

    #[test]
    fn converts_one_based_ranges_to_zero_based_lsp_positions() {
        let tour = parse(
            r#"# Columns

## 1. Character columns
[target](src/main.rs#L3:7-L4:2)
"#,
        )
        .expect("tour should parse");

        let range = tour.steps[0].target.range;
        assert_eq!(range.start, TourPosition { line: 3, column: 7 });
        assert_eq!(range.end, TourPosition { line: 4, column: 2 });
        assert_eq!(
            range.to_lsp_range(),
            Range::new(Position::new(2, 6), Position::new(3, 1))
        );
    }

    #[test]
    fn preserves_step_order_from_the_document() {
        let tour = parse(
            r#"# Ordered

## 2. Second
[second](src/second.rs#L2:2-L2:10)

Second body.

## 1. First
[first](src/first.rs#L1:1-L1:5)

First body.
"#,
        )
        .expect("tour should parse");

        assert_eq!(tour.steps.len(), 2);
        assert_eq!(tour.steps[0].number, 2);
        assert_eq!(tour.steps[0].title, "Second");
        assert_eq!(tour.steps[0].target.path, Path::new("/tours/src/second.rs"));
        assert_eq!(tour.steps[1].number, 1);
        assert_eq!(tour.steps[1].title, "First");
        assert_eq!(tour.steps[1].target.path, Path::new("/tours/src/first.rs"));
    }

    #[test]
    fn resolves_relative_and_absolute_paths() {
        let tour = parse(
            r#"# Paths

## 1. Relative
[relative](src/nested/main.rs#L1:1-L1:2)

## 2. Absolute
[absolute](/workspace/project/src/lib.rs#L3:4-L5:6)
"#,
        )
        .expect("tour should parse");

        assert_eq!(
            tour.steps[0].target.path,
            Path::new("/tours/src/nested/main.rs")
        );
        assert_eq!(
            tour.steps[1].target.path,
            Path::new("/workspace/project/src/lib.rs")
        );
        assert_eq!(
            tour.steps[1].target.raw_path,
            "/workspace/project/src/lib.rs"
        );
    }

    #[test]
    fn uses_the_first_link_in_a_step_as_the_target() {
        let tour = parse(
            r#"# First link

## 1. Entry
[first](src/first.rs#L1:1-L1:5)

[second](src/second.rs#L2:2-L2:8)
"#,
        )
        .expect("tour should parse");

        assert_eq!(tour.steps[0].target.path, Path::new("/tours/src/first.rs"));
        assert_eq!(tour.steps[0].target.range.start.line, 1);
    }

    #[test]
    fn hint_metadata_overrides_the_first_paragraph() {
        let tour = parse(
            r#"# Hints

## 1. Entry
[target](src/main.rs#L1:1-L1:5)

<!-- tour-step: {"hint": "Short hidden hint"} -->

Visible first paragraph.

Second paragraph.
"#,
        )
        .expect("tour should parse");

        let step = &tour.steps[0];
        assert_eq!(step.hint.as_deref(), Some("Short hidden hint"));
        assert_eq!(step.body_hint(), "Short hidden hint");
        assert_eq!(step.body, "Visible first paragraph.\n\nSecond paragraph.");
    }

    #[test]
    fn ignores_links_inside_hidden_metadata() {
        let tour = parse(
            r#"# Hidden links

## 1. Entry
<!-- tour-step: {"hint": "See [docs](docs.rs#L1:1-L1:2)"} -->
[target](src/main.rs#L1:1-L1:5)

Visible body.
"#,
        )
        .expect("tour should parse");

        let step = &tour.steps[0];
        assert_eq!(step.target.path, Path::new("/tours/src/main.rs"));
        assert_eq!(step.hint.as_deref(), Some("See [docs](docs.rs#L1:1-L1:2)"));
        assert_eq!(step.body, "Visible body.");
    }

    #[test]
    fn drops_html_comments_from_the_body() {
        let tour = parse(
            r#"# Comments

## 1. Entry
[target](src/main.rs#L1:1-L1:5)

<!-- a regular Markdown comment -->
Visible body.
"#,
        )
        .expect("tour should parse");

        assert_eq!(tour.steps[0].body, "Visible body.");
        assert_eq!(tour.steps[0].body_hint(), "Visible body.");
    }

    #[test]
    fn falls_back_to_the_first_body_paragraph() {
        let tour = parse(
            r#"# Paragraphs

## 1. Entry
[target](src/main.rs#L1:1-L1:5)

First line
second line.

Next paragraph.
"#,
        )
        .expect("tour should parse");

        assert_eq!(tour.steps[0].body_hint(), "First line\nsecond line.");
    }

    #[test]
    fn allows_an_empty_step_body() {
        let tour = parse(
            r#"# Empty body

## 1. Entry
[target](src/main.rs#L1:1-L1:5)
"#,
        )
        .expect("tour should parse");

        assert_eq!(tour.steps[0].body, "");
        assert_eq!(tour.steps[0].hint, None);
        assert_eq!(tour.steps[0].body_hint(), "");
    }

    #[test]
    fn reports_a_step_without_a_target_link() {
        let errors = errors(
            r#"# Missing link

## 1. Entry

This step has no target link.
"#,
        );

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ParseErrorKind::MissingStepLink);
        assert_eq!(errors[0].line, Some(3));
    }

    #[test]
    fn reports_a_malformed_step_heading() {
        let errors = errors(
            r#"# Bad heading

## Entry point
[target](src/main.rs#L1:1-L1:5)
"#,
        );

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ParseErrorKind::MalformedStepHeading);
        assert_eq!(errors[0].line, Some(3));
    }

    #[test]
    fn reports_a_missing_title() {
        let kinds = error_kinds(
            r#"## 1. Entry
[target](src/main.rs#L1:1-L1:5)
"#,
        );

        assert!(kinds.contains(&ParseErrorKind::MissingTitle));
    }

    #[test]
    fn reports_a_malformed_title() {
        let kinds = error_kinds(
            r#"# 
## 1. Entry
[target](src/main.rs#L1:1-L1:5)
"#,
        );

        assert!(kinds.contains(&ParseErrorKind::MalformedTitle));
    }

    #[test]
    fn reports_malformed_and_invalid_ranges() {
        let cases = [
            ("src/main.rs#L12-L20", ParseErrorKind::MalformedRange),
            ("src/main.rs#L12:4-20:1", ParseErrorKind::MalformedRange),
            ("src/main.rs#L12:4", ParseErrorKind::MalformedRange),
            ("src/main.rs#L0:1-L1:2", ParseErrorKind::InvalidRange),
            ("src/main.rs#L12:4-L10:1", ParseErrorKind::InvalidRange),
            ("src/main.rs#L12:4-L12:3", ParseErrorKind::InvalidRange),
            ("src/main.rs#section", ParseErrorKind::MalformedRange),
            ("src/main.rs", ParseErrorKind::MissingRangeFragment),
        ];

        for (target, expected_kind) in cases {
            let source = format!(
                r#"# Ranges

## 1. Entry
[target]({target})
"#
            );
            let kinds = error_kinds(&source);
            assert_eq!(
                kinds,
                vec![expected_kind],
                "unexpected parse result for {target}"
            );
        }
    }

    #[test]
    fn rejects_non_file_targets() {
        let cases = ["zed:///agent/thread/session-1#L1:1-L1:2", "#L1:1-L1:2"];

        for target in cases {
            let source = format!(
                r#"# Unsupported

## 1. Entry
[target]({target})
"#
            );
            let errors = errors(&source);
            assert_eq!(errors.len(), 1, "unexpected parse result for {target}");
            assert_eq!(errors[0].kind, ParseErrorKind::UnsupportedTarget);
            assert_eq!(errors[0].line, Some(4));
        }
    }

    #[test]
    fn reports_malformed_hint_metadata() {
        let cases = [
            r#"<!-- tour-step: {"hint": } -->"#,
            r#"<!-- tour-step: {"hint": 4} -->"#,
            r#"<!-- tour-step: [] -->"#,
            r#"<!-- tour-step: {"hint": "unterminated" }"#,
        ];

        for metadata in cases {
            let source = format!(
                r#"# Bad hint

## 1. Entry
[target](src/main.rs#L1:1-L1:5)

{metadata}
"#
            );
            let errors = errors(&source);
            assert!(
                errors
                    .iter()
                    .any(|error| error.kind == ParseErrorKind::MalformedHintMetadata),
                "expected malformed hint metadata for {metadata}"
            );
        }
    }
}
