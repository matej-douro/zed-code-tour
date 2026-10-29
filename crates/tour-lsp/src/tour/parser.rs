use std::{
    collections::HashSet,
    fmt,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::uri::normalize_path;

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
    DuplicateStepNumber,
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
/// first one, and never panics on malformed input. Fenced code blocks and HTML
/// comments are treated as Markdown structure, so their contents cannot become
/// tour headings, target links, or hint metadata.
pub fn parse_tour(tour_path: impl AsRef<Path>, source: &str) -> Result<Tour, Vec<ParseError>> {
    let tour_path = tour_path.as_ref();
    let base_dir = tour_path.parent().unwrap_or_else(|| Path::new(""));

    let source_lines = preprocess(source);
    let mut errors = Vec::new();
    let mut title: Option<String> = None;
    let mut thread_link: Option<ThreadLink> = None;
    let mut steps = Vec::new();
    let mut seen_step_numbers = HashSet::new();
    let mut current_step: Option<StepBuilder> = None;

    for line in source_lines {
        if line.fenced {
            if let Some(step) = current_step.as_mut() {
                step.lines.push(line);
            }
            continue;
        }

        if thread_link.is_none()
            && let Some(link) = find_markdown_links(&line.visible)
                .into_iter()
                .find(|link| link.destination.starts_with(THREAD_URI_PREFIX))
        {
            thread_link = Some(ThreadLink {
                text: link.text,
                uri: link.destination,
                line: line.number,
            });
        }

        let Some(trimmed) = structural_text(&line) else {
            if let Some(step) = current_step.as_mut() {
                step.lines.push(line);
            }
            continue;
        };

        if let Some(rest) = trimmed.strip_prefix("## ") {
            finish_step(current_step.take(), base_dir, &mut errors, &mut steps);

            match parse_step_heading(rest) {
                Ok((number, step_title)) => {
                    if !seen_step_numbers.insert(number) {
                        errors.push(ParseError::new(
                            Some(line.number),
                            ParseErrorKind::DuplicateStepNumber,
                            format!("step number {number} is used more than once"),
                        ));
                    }
                    current_step = Some(StepBuilder {
                        number,
                        title: step_title,
                        heading_line: line.number,
                        lines: Vec::new(),
                    });
                }
                Err(message) => errors.push(ParseError::new(
                    Some(line.number),
                    ParseErrorKind::MalformedStepHeading,
                    message,
                )),
            }
            continue;
        }

        if trimmed.starts_with("##") && !trimmed.starts_with("###") {
            finish_step(current_step.take(), base_dir, &mut errors, &mut steps);
            errors.push(ParseError::new(
                Some(line.number),
                ParseErrorKind::MalformedStepHeading,
                format!("malformed step heading `{}`", trimmed.trim_end()),
            ));
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("# ") {
            if let Some(step) = current_step.as_mut() {
                step.lines.push(line);
            } else if title.is_none() {
                let parsed_title = strip_atx_closing(rest);
                if parsed_title.is_empty() {
                    errors.push(ParseError::new(
                        Some(line.number),
                        ParseErrorKind::MalformedTitle,
                        "tour title must not be empty",
                    ));
                } else {
                    title = Some(parsed_title.to_owned());
                }
            }
            continue;
        }

        if trimmed == "#" {
            if let Some(step) = current_step.as_mut() {
                step.lines.push(line);
            } else if title.is_none() {
                errors.push(ParseError::new(
                    Some(line.number),
                    ParseErrorKind::MalformedTitle,
                    "tour title must not be empty",
                ));
            }
            continue;
        }

        if let Some(step) = current_step.as_mut() {
            step.lines.push(line);
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
    lines: Vec<SourceLine>,
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
    let mut hint = None;

    for line in &builder.lines {
        if line.fenced {
            continue;
        }

        if target_link.is_none()
            && let Some(link) = find_markdown_links(&line.visible)
                .into_iter()
                .find(|link| !link.destination.starts_with(THREAD_URI_PREFIX))
        {
            target_link = Some((line.number, link));
        }

        for comment in &line.comments {
            match parse_step_metadata(comment) {
                Ok(Some(comment_hint)) => {
                    if hint.is_none() {
                        hint = Some(comment_hint);
                    }
                }
                Ok(None) => {}
                Err(message) => errors.push(ParseError::new(
                    Some(line.number),
                    ParseErrorKind::MalformedHintMetadata,
                    message,
                )),
            }
        }
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
    for line in &builder.lines {
        if line.fenced {
            body_lines.push(line.visible.clone());
            continue;
        }

        let mut spans = Vec::new();
        if line.number == target_line {
            spans.push((target_link.start, target_link.end));
        }
        for link in find_markdown_links(&line.visible) {
            if link.destination.starts_with(THREAD_URI_PREFIX) {
                spans.push((link.start, link.end));
            }
        }

        spans.sort_unstable_by_key(|span| std::cmp::Reverse(span.0));
        let mut cleaned = line.visible.clone();
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
        .map(strip_atx_closing)
        .filter(|title| !title.is_empty())
        .ok_or_else(|| format!("malformed step heading `## {rest}`"))?;

    Ok((number, title.to_owned()))
}

fn strip_atx_closing(title: &str) -> &str {
    let title = title.trim();
    let without_hashes = title.trim_end_matches('#');
    if without_hashes.len() == title.len() || without_hashes.is_empty() {
        return title;
    }

    if without_hashes
        .chars()
        .next_back()
        .is_some_and(char::is_whitespace)
    {
        without_hashes.trim_end()
    } else {
        title
    }
}

fn structural_text(line: &SourceLine) -> Option<&str> {
    let trimmed = line.visible.trim_start();
    if trimmed.is_empty() {
        return None;
    }

    let visible_start = line.visible.len() - trimmed.len();
    if line.text[..visible_start].chars().all(char::is_whitespace) {
        Some(trimmed)
    } else {
        None
    }
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
    let path = normalize_path(&path);

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

fn find_markdown_links(text: &str) -> Vec<MarkdownLink> {
    let mut links = Vec::new();
    let mut search_start = 0;

    while search_start < text.len() {
        let Some(offset) = text[search_start..].find('[') else {
            break;
        };
        let index = search_start + offset;

        if let Some(link) = markdown_link_at(text, index) {
            search_start = link.end;
            links.push(link);
        } else {
            search_start = index + 1;
        }
    }

    links
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
    let close_paren = find_closing_paren(bytes, close_bracket + 2)?;
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

fn find_closing_paren(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0usize;

    for (index, byte) in bytes.iter().enumerate().skip(start) {
        match byte {
            b'(' => depth += 1,
            b')' if depth == 0 => return Some(index),
            b')' => depth -= 1,
            _ => {}
        }
    }

    None
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

struct SourceLine {
    number: u32,
    text: String,
    visible: String,
    fenced: bool,
    comments: Vec<CommentBlock>,
}

struct CommentBlock {
    content: String,
    closed: bool,
}

struct ActiveComment {
    start_line_index: usize,
    content: String,
}

fn preprocess(source: &str) -> Vec<SourceLine> {
    let mut lines = Vec::new();
    let mut fence: Option<Fence> = None;
    let mut active_comment: Option<ActiveComment> = None;

    for (index, raw_line) in source.lines().enumerate() {
        let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let text = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let line_index = lines.len();
        let mut visible = text.to_owned();
        let mut completed_comments = Vec::new();

        if let Some(open_fence) = fence {
            if open_fence.closes(text.trim_start()) {
                fence = None;
            }
            lines.push(SourceLine {
                number: line_number,
                text: text.to_owned(),
                visible,
                fenced: true,
                comments: Vec::new(),
            });
            continue;
        }

        if active_comment.is_none()
            && let Some(open_fence) = Fence::opening(text)
        {
            fence = Some(open_fence);
            lines.push(SourceLine {
                number: line_number,
                text: text.to_owned(),
                visible,
                fenced: true,
                comments: Vec::new(),
            });
            continue;
        }

        process_comments(
            text,
            &mut visible,
            &mut active_comment,
            line_index,
            &mut completed_comments,
        );

        lines.push(SourceLine {
            number: line_number,
            text: text.to_owned(),
            visible,
            fenced: false,
            comments: Vec::new(),
        });
        for (start_index, comment) in completed_comments {
            lines[start_index].comments.push(comment);
        }
    }

    if let Some(comment) = active_comment {
        lines[comment.start_line_index].comments.push(CommentBlock {
            content: comment.content,
            closed: false,
        });
    }

    lines
}

fn process_comments(
    text: &str,
    visible: &mut String,
    active_comment: &mut Option<ActiveComment>,
    line_index: usize,
    completed_comments: &mut Vec<(usize, CommentBlock)>,
) {
    if let Some(comment) = active_comment.as_mut() {
        comment.content.push('\n');
    }

    let mut position = 0;
    loop {
        if let Some(comment) = active_comment.as_mut() {
            if let Some(offset) = text[position..].find("-->") {
                let comment_end = position + offset;
                comment.content.push_str(&text[position..comment_end]);
                mask_range(visible, position, comment_end + 3);

                let comment = active_comment.take().expect("active comment");
                completed_comments.push((
                    comment.start_line_index,
                    CommentBlock {
                        content: comment.content,
                        closed: true,
                    },
                ));
                position = comment_end + 3;
                continue;
            }

            comment.content.push_str(&text[position..]);
            mask_range(visible, position, text.len());
            break;
        }

        let Some(offset) = text[position..].find("<!--") else {
            break;
        };
        let comment_start = position + offset;
        let content_start = comment_start + 4;

        if let Some(close_offset) = text[content_start..].find("-->") {
            let content_end = content_start + close_offset;
            completed_comments.push((
                line_index,
                CommentBlock {
                    content: text[content_start..content_end].to_owned(),
                    closed: true,
                },
            ));
            mask_range(visible, comment_start, content_end + 3);
            position = content_end + 3;
        } else {
            *active_comment = Some(ActiveComment {
                start_line_index: line_index,
                content: text[content_start..].to_owned(),
            });
            mask_range(visible, comment_start, text.len());
            break;
        }
    }
}

fn mask_range(text: &mut String, start: usize, end: usize) {
    if start < end && end <= text.len() {
        text.replace_range(start..end, &" ".repeat(end - start));
    }
}

fn parse_step_metadata(comment: &CommentBlock) -> Result<Option<String>, String> {
    let content = comment.content.trim_start();
    let Some(data) = content.strip_prefix(TOUR_STEP_METADATA_PREFIX) else {
        return Ok(None);
    };

    if !comment.closed {
        return Err(format!(
            "unterminated `{TOUR_STEP_METADATA_PREFIX}` metadata comment"
        ));
    }

    let value: Value = serde_json::from_str(data.trim())
        .map_err(|error| format!("invalid tour-step metadata JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "tour-step metadata must be a JSON object".to_owned())?;

    match object.get("hint") {
        None => Ok(None),
        Some(Value::String(hint)) => Ok(Some(hint.clone())),
        Some(_) => Err("tour-step metadata `hint` must be a string".to_owned()),
    }
}

#[derive(Clone, Copy)]
struct Fence {
    marker: char,
    length: usize,
}

impl Fence {
    fn opening(line: &str) -> Option<Self> {
        let trimmed = line.trim_start_matches(' ');
        if line.len() - trimmed.len() > 3 {
            return None;
        }

        let marker = trimmed.chars().next()?;
        if marker != '`' && marker != '~' {
            return None;
        }

        let length = trimmed
            .chars()
            .take_while(|character| *character == marker)
            .count();
        if length < 3 || (marker == '`' && trimmed[length..].contains('`')) {
            return None;
        }

        Some(Self { marker, length })
    }

    fn closes(self, line: &str) -> bool {
        let trimmed = line.trim_start_matches(' ');
        if line.len() - trimmed.len() > 3 || !trimmed.starts_with(self.marker) {
            return false;
        }

        let length = trimmed
            .chars()
            .take_while(|character| *character == self.marker)
            .count();
        length >= self.length && trimmed[length..].trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

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
                line: 3,
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
    fn preserves_one_based_character_ranges_from_the_link() {
        let tour = parse(
            r#"# Columns

## 1. Character columns
[target](src/main.rs#L3:7-L4:2)
"#,
        )
        .expect("tour should parse");

        assert_eq!(
            tour.steps[0].target.range,
            TourRange {
                start: TourPosition { line: 3, column: 7 },
                end: TourPosition { line: 4, column: 2 },
            }
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
    fn normalizes_dot_segments_in_target_paths() {
        let tour = parse_tour(
            Path::new("/tours/examples/example.tour.md"),
            r#"# Paths

## 1. Parent
[parent](../crates/tour-lsp/src/main.rs#L1:1-L1:2)

## 2. Current
[current](./src/./nested/../main.rs#L1:1-L1:2)

## 3. Absolute
[absolute](/tours/./examples/../crates/main.rs#L1:1-L1:2)
"#,
        )
        .expect("tour should parse");

        assert_eq!(
            tour.steps[0].target.raw_path,
            "../crates/tour-lsp/src/main.rs"
        );
        assert_eq!(
            tour.steps[0].target.path,
            Path::new("/tours/crates/tour-lsp/src/main.rs")
        );
        assert_eq!(tour.steps[1].target.raw_path, "./src/./nested/../main.rs");
        assert_eq!(
            tour.steps[1].target.path,
            Path::new("/tours/examples/src/main.rs")
        );
        assert_eq!(
            tour.steps[2].target.path,
            Path::new("/tours/crates/main.rs")
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
        let cases = ["file:///tmp/src.rs#L1:1-L1:2", "#L1:1-L1:2"];

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
    #[test]
    fn ignores_tour_syntax_inside_fenced_code_blocks() {
        let tour = parse(
            r#"# Fenced

## 1. Entry
[target](src/main.rs#L1:1-L1:5)

Visible body.

```rust
## 2. Not a step
[fake](fake.rs#L1:1-L1:2)
<!-- tour-step: {"hint": "fake hint"} -->
```
"#,
        )
        .expect("tour should parse");

        assert_eq!(tour.steps.len(), 1);
        assert_eq!(tour.steps[0].title, "Entry");
        assert_eq!(tour.steps[0].target.path, Path::new("/tours/src/main.rs"));
        assert_eq!(tour.steps[0].hint, None);
        assert_eq!(tour.steps[0].body_hint(), "Visible body.");
        assert!(tour.steps[0].body.contains("## 2. Not a step"));
        assert!(tour.steps[0].body.contains("<!-- tour-step:"));
    }

    #[test]
    fn hides_multiline_html_comments_from_the_body() {
        let tour = parse(
            r#"# Comments

## 1. Entry
[target](src/main.rs#L1:1-L1:5)

<!-- start
still hidden -->
visible body
"#,
        )
        .expect("tour should parse");

        assert_eq!(tour.steps[0].body_hint(), "visible body");
        assert!(!tour.steps[0].body.contains("still hidden"));
        assert!(!tour.steps[0].body.contains("-->"));
    }

    #[test]
    fn parses_multiline_tour_step_metadata() {
        let tour = parse(
            r#"# Hints

## 1. Entry
[target](src/main.rs#L1:1-L1:5)

<!-- tour-step: {
  "hint": "Multi-line hint"
} -->

Visible body.
"#,
        )
        .expect("tour should parse");

        assert_eq!(tour.steps[0].hint.as_deref(), Some("Multi-line hint"));
        assert_eq!(tour.steps[0].body_hint(), "Multi-line hint");
        assert_eq!(tour.steps[0].body, "Visible body.");
    }

    #[test]
    fn parses_link_destinations_with_balanced_parentheses() {
        let tour = parse(
            r#"# Parentheses

## 1. Entry
[target](dir/a(1).rs#L1:1-L1:5)
"#,
        )
        .expect("tour should parse");

        assert_eq!(tour.steps[0].target.raw_path, "dir/a(1).rs");
        assert_eq!(tour.steps[0].target.path, Path::new("/tours/dir/a(1).rs"));
    }

    #[test]
    fn finds_a_thread_link_that_is_not_the_first_link() {
        let tour = parse(
            r#"# Thread

[other](https://example.com) [thread](zed:///agent/thread/session-1?name=Auth)

## 1. Entry
[target](src/main.rs#L1:1-L1:5)
"#,
        )
        .expect("tour should parse");

        assert_eq!(
            tour.thread_link,
            Some(ThreadLink {
                text: "thread".to_owned(),
                uri: "zed:///agent/thread/session-1?name=Auth".to_owned(),
                line: 3,
            })
        );
    }

    #[test]
    fn finds_a_thread_link_after_the_steps() {
        let tour = parse(
            r#"# Thread

## 1. Entry
[target](src/main.rs#L1:1-L1:5)

[thread](zed:///agent/thread/session-1)
"#,
        )
        .expect("tour should parse");

        assert_eq!(
            tour.thread_link,
            Some(ThreadLink {
                text: "thread".to_owned(),
                uri: "zed:///agent/thread/session-1".to_owned(),
                line: 6,
            })
        );
        assert_eq!(tour.steps[0].body, "");
    }

    #[test]
    fn reports_duplicate_step_numbers() {
        let errors = errors(
            r#"# Duplicates

## 1. First
[first](src/first.rs#L1:1-L1:2)

## 1. Second
[second](src/second.rs#L1:1-L1:2)
"#,
        );

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ParseErrorKind::DuplicateStepNumber);
        assert_eq!(errors[0].line, Some(6));
    }

    #[test]
    fn strips_closed_atx_hashes_from_titles() {
        let tour = parse(
            r#"# Title #

## 1. Step title ##
[target](src/main.rs#L1:1-L1:5)
"#,
        )
        .expect("tour should parse");

        assert_eq!(tour.title, "Title");
        assert_eq!(tour.steps[0].title, "Step title");
    }

    #[test]
    fn reports_a_bare_hash_as_a_malformed_title() {
        let kinds = error_kinds(
            r#"#
## 1. Entry
[target](src/main.rs#L1:1-L1:5)
"#,
        );

        assert!(kinds.contains(&ParseErrorKind::MalformedTitle));
    }

    #[test]
    fn removes_a_target_link_with_a_comment_inside_its_text() {
        let tour = parse(
            r#"# Overlap

## 1. Entry
[ab<!--x-->cd](src/main.rs#L1:1-L1:5)

Visible body.
"#,
        )
        .expect("tour should parse");

        assert_eq!(tour.steps[0].target.path, Path::new("/tours/src/main.rs"));
        assert_eq!(tour.steps[0].body, "Visible body.");
        assert!(!tour.steps[0].body.contains("src/main.rs"));
    }
    #[test]
    fn ignores_a_thread_link_when_choosing_a_step_target() {
        let tour = parse(
            r#"# Thread first

## 1. Entry
[thread](zed:///agent/thread/session-1)
[target](src/main.rs#L1:1-L1:5)

Visible body.
"#,
        )
        .expect("tour should parse");

        assert_eq!(tour.steps[0].target.path, Path::new("/tours/src/main.rs"));
        assert_eq!(
            tour.thread_link,
            Some(ThreadLink {
                text: "thread".to_owned(),
                uri: "zed:///agent/thread/session-1".to_owned(),
                line: 4,
            })
        );
        assert_eq!(tour.steps[0].body, "Visible body.");
    }
}
