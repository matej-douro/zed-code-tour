use std::fmt;

use lsp_types::{Position, Range, ShowDocumentParams, Uri};

use crate::tour::{TourPosition, TourRange};

/// Convert a 1-based tour position to a zero-based LSP position.
///
/// Tour columns count Unicode scalar values. LSP columns count UTF-16 code
/// units, so a column before an astral-plane character maps to a smaller UTF-16
/// offset than its scalar index. Line endings are not part of a line, so CRLF
/// input counts the same as LF input.
pub fn tour_position_to_lsp(text: &str, position: TourPosition) -> Result<Position, PositionError> {
    let lines = source_lines(text);
    let line = line_at(&lines, position.line)?;
    let character = character_offset(line, position)?;

    Ok(Position::new(
        position.line - 1,
        u32::try_from(character).unwrap_or(u32::MAX),
    ))
}

/// Convert a zero-based UTF-16 LSP position back to a 1-based tour position.
///
/// This is the inverse of [`tour_position_to_lsp`] and is used by diagnostics
/// and tests. A position that splits a surrogate pair is rejected instead of
/// being rounded.
pub fn lsp_position_to_tour(text: &str, position: Position) -> Result<TourPosition, PositionError> {
    let lines = source_lines(text);
    let line = line_at(&lines, position.line.saturating_add(1))?;
    let mut utf16 = 0u32;

    for (index, character) in line.chars().enumerate() {
        if utf16 == position.character {
            return Ok(TourPosition {
                line: position.line + 1,
                column: u32::try_from(index).unwrap_or(u32::MAX) + 1,
            });
        }

        utf16 = utf16.saturating_add(u32::try_from(character.len_utf16()).unwrap_or(1));
        if utf16 > position.character {
            return Err(PositionError::Utf16Split {
                line: position.line + 1,
                character: position.character,
            });
        }
    }

    if utf16 == position.character {
        return Ok(TourPosition {
            line: position.line + 1,
            column: u32::try_from(line.chars().count()).unwrap_or(u32::MAX) + 1,
        });
    }

    Err(PositionError::LspColumnOutOfRange {
        line: position.line + 1,
        character: position.character,
        max_character: utf16,
    })
}

/// Convert a 1-based tour range to a zero-based LSP range with UTF-16 columns.
pub fn tour_range_to_lsp(text: &str, range: TourRange) -> Result<Range, PositionError> {
    Ok(Range::new(
        tour_position_to_lsp(text, range.start)?,
        tour_position_to_lsp(text, range.end)?,
    ))
}

/// Convert a zero-based UTF-16 LSP range back to a 1-based tour range.
pub fn lsp_range_to_tour(text: &str, range: Range) -> Result<TourRange, PositionError> {
    Ok(TourRange {
        start: lsp_position_to_tour(text, range.start)?,
        end: lsp_position_to_tour(text, range.end)?,
    })
}

/// Build the `window/showDocument` parameters for a tour step.
pub fn show_document_params(uri: Uri, range: Range) -> ShowDocumentParams {
    ShowDocumentParams {
        uri,
        external: Some(false),
        take_focus: Some(true),
        selection: Some(range),
    }
}

fn line_at<'a>(lines: &'a [&'a str], line: u32) -> Result<&'a str, PositionError> {
    let index = match line.checked_sub(1) {
        Some(index) => usize::try_from(index).unwrap_or(usize::MAX),
        None => {
            return Err(PositionError::LineOutOfRange {
                line,
                line_count: lines.len(),
            });
        }
    };
    lines
        .get(index)
        .copied()
        .ok_or(PositionError::LineOutOfRange {
            line,
            line_count: lines.len(),
        })
}

fn character_offset(line: &str, position: TourPosition) -> Result<usize, PositionError> {
    let character_count = line.chars().count();
    let index = match position.column.checked_sub(1) {
        Some(index) => usize::try_from(index).unwrap_or(usize::MAX),
        None => {
            return Err(PositionError::TourColumnOutOfRange {
                line: position.line,
                column: position.column,
                max_column: u32::try_from(character_count).unwrap_or(u32::MAX) + 1,
            });
        }
    };
    if index > character_count {
        return Err(PositionError::TourColumnOutOfRange {
            line: position.line,
            column: position.column,
            max_column: u32::try_from(character_count).unwrap_or(u32::MAX) + 1,
        });
    }

    Ok(line
        .chars()
        .take(index)
        .map(|character| character.len_utf16())
        .sum())
}

fn source_lines(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// A tour position that cannot be represented in the current source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositionError {
    LineOutOfRange {
        line: u32,
        line_count: usize,
    },
    TourColumnOutOfRange {
        line: u32,
        column: u32,
        max_column: u32,
    },
    LspColumnOutOfRange {
        line: u32,
        character: u32,
        max_character: u32,
    },
    Utf16Split {
        line: u32,
        character: u32,
    },
}

impl fmt::Display for PositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LineOutOfRange { line, line_count } => write!(
                f,
                "line {line} is outside the source file, which has {line_count} line(s)"
            ),
            Self::TourColumnOutOfRange {
                line,
                column,
                max_column,
            } => write!(
                f,
                "tour column {column} is outside line {line}, which ends at column {max_column}"
            ),
            Self::LspColumnOutOfRange {
                line,
                character,
                max_character,
            } => write!(
                f,
                "LSP character {character} is outside line {line}, which ends at character {max_character}"
            ),
            Self::Utf16Split { line, character } => write!(
                f,
                "LSP character {character} on line {line} splits a UTF-16 surrogate pair"
            ),
        }
    }
}

impl std::error::Error for PositionError {}

#[cfg(test)]
mod tests {
    use lsp_types::{Position, Range};

    use super::{
        PositionError, lsp_position_to_tour, lsp_range_to_tour, show_document_params,
        tour_position_to_lsp, tour_range_to_lsp,
    };
    use crate::tour::{TourPosition, TourRange};

    fn tour_position(line: u32, column: u32) -> TourPosition {
        TourPosition { line, column }
    }

    fn tour_range(start_line: u32, start_column: u32, end_line: u32, end_column: u32) -> TourRange {
        TourRange {
            start: tour_position(start_line, start_column),
            end: tour_position(end_line, end_column),
        }
    }

    #[test]
    fn maps_ascii_columns_and_end_of_line() {
        let text = "fn main() {}\nlet value = 1;\n";

        assert_eq!(
            tour_position_to_lsp(text, tour_position(1, 1)).expect("start"),
            Position::new(0, 0)
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(1, 4)).expect("inside line"),
            Position::new(0, 3)
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(1, 13)).expect("end of line"),
            Position::new(0, 12)
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(2, 15)).expect("end of line"),
            Position::new(1, 14)
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(3, 1)).expect("trailing empty line"),
            Position::new(2, 0)
        );
    }

    #[test]
    fn maps_multibyte_utf8_as_utf16_code_units() {
        let text = "aé中b";

        assert_eq!(
            tour_position_to_lsp(text, tour_position(1, 1)).expect("a"),
            Position::new(0, 0)
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(1, 2)).expect("é"),
            Position::new(0, 1)
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(1, 3)).expect("中"),
            Position::new(0, 2)
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(1, 4)).expect("b"),
            Position::new(0, 3)
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(1, 5)).expect("end"),
            Position::new(0, 4)
        );
    }

    #[test]
    fn maps_astral_plane_characters_as_two_utf16_code_units() {
        let text = "a😀b";

        assert_eq!(
            tour_position_to_lsp(text, tour_position(1, 2)).expect("emoji"),
            Position::new(0, 1)
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(1, 3)).expect("b"),
            Position::new(0, 3)
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(1, 4)).expect("end"),
            Position::new(0, 4)
        );

        assert_eq!(
            lsp_position_to_tour(text, Position::new(0, 3)).expect("after emoji"),
            tour_position(1, 3)
        );
        assert_eq!(
            lsp_position_to_tour(text, Position::new(0, 2)),
            Err(PositionError::Utf16Split {
                line: 1,
                character: 2,
            })
        );
    }

    #[test]
    fn maps_crlf_input_without_counting_carriage_returns() {
        let text = "aé\r\nb😀\r\n";

        assert_eq!(
            tour_position_to_lsp(text, tour_position(1, 3)).expect("end of first line"),
            Position::new(0, 2)
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(2, 2)).expect("emoji"),
            Position::new(1, 1)
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(2, 3)).expect("end of second line"),
            Position::new(1, 3)
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(2, 4)),
            Err(PositionError::TourColumnOutOfRange {
                line: 2,
                column: 4,
                max_column: 3,
            })
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(1, 4)),
            Err(PositionError::TourColumnOutOfRange {
                line: 1,
                column: 4,
                max_column: 3,
            })
        );
    }

    #[test]
    fn maps_ranges_that_span_lines() {
        let text = "first 😀 line\r\nsecond é line\r\nthird\n";
        let range = tour_range(1, 7, 2, 8);

        let lsp_range = tour_range_to_lsp(text, range).expect("range should map");
        assert_eq!(
            lsp_range,
            Range::new(Position::new(0, 6), Position::new(1, 7))
        );
        assert_eq!(
            lsp_range_to_tour(text, lsp_range).expect("range should round trip"),
            range
        );
    }

    #[test]
    fn rejects_lines_columns_and_utf16_splits_outside_the_source() {
        let text = "a😀b";

        assert_eq!(
            lsp_position_to_tour(text, Position::new(1, 0)),
            Err(PositionError::LineOutOfRange {
                line: 2,
                line_count: 1,
            })
        );
        assert_eq!(
            lsp_position_to_tour(text, Position::new(0, 9)),
            Err(PositionError::LspColumnOutOfRange {
                line: 1,
                character: 9,
                max_character: 4,
            })
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(2, 1)),
            Err(PositionError::LineOutOfRange {
                line: 2,
                line_count: 1,
            })
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(0, 1)),
            Err(PositionError::LineOutOfRange {
                line: 0,
                line_count: 1,
            })
        );
        assert_eq!(
            tour_position_to_lsp(text, tour_position(1, 0)),
            Err(PositionError::TourColumnOutOfRange {
                line: 1,
                column: 0,
                max_column: 4,
            })
        );
    }

    #[test]
    fn builds_a_focused_show_document_selection() {
        let params = show_document_params(
            "file:///tours/src/main.rs".parse().expect("valid URI"),
            Range::new(Position::new(1, 4), Position::new(1, 9)),
        );

        assert_eq!(params.uri.as_str(), "file:///tours/src/main.rs");
        assert_eq!(params.external, Some(false));
        assert_eq!(params.take_focus, Some(true));
        assert_eq!(
            params.selection,
            Some(Range::new(Position::new(1, 4), Position::new(1, 9)))
        );
    }
}
