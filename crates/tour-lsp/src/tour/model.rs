use std::path::PathBuf;

/// A parsed `.tour.md` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tour {
    pub title: String,
    pub thread_link: Option<ThreadLink>,
    pub steps: Vec<TourStep>,
}

/// The optional link back to the agent thread that created the tour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadLink {
    pub text: String,
    pub uri: String,
    /// 1-based line in the tour file that contains the link.
    pub line: u32,
}

/// One ordered tour step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TourStep {
    pub number: u32,
    pub title: String,
    pub target: TourTarget,
    /// Markdown body with the target link and hidden metadata removed.
    pub body: String,
    /// Explicit plain-text hint from `<!-- tour-step: {"hint": "..."} -->`.
    pub hint: Option<String>,
}

impl TourStep {
    /// Plain-text body hint: the explicit metadata first, then the first
    /// paragraph of the visible body.
    pub fn body_hint(&self) -> &str {
        match &self.hint {
            Some(hint) => hint,
            None => first_paragraph(&self.body),
        }
    }
}

/// The first link in a step, resolved against the tour file directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TourTarget {
    pub link_text: String,
    /// Path exactly as written in the link.
    pub raw_path: String,
    /// Path resolved against the tour file directory, or kept as absolute.
    pub path: PathBuf,
    /// 1-based line and column range from the link fragment.
    pub range: TourRange,
}

/// A 1-based file position, matching the tour file link format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TourPosition {
    pub line: u32,
    pub column: u32,
}

/// A 1-based start/end range from a `#L<line>:<column>-L<line>:<column>` fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TourRange {
    pub start: TourPosition,
    pub end: TourPosition,
}

fn first_paragraph(body: &str) -> &str {
    let body = body.trim();
    if body.is_empty() {
        return body;
    }

    let mut end = body.len();
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        if line.trim().is_empty() {
            end = offset;
            break;
        }
        offset += line.len();
    }

    body[..end].trim_end()
}
