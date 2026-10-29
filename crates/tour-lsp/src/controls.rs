use lsp_types::{CodeLens, Command, Position, Range};

use crate::{hint::source_line_count, settings::Placement};

pub const PREVIOUS_COMMAND: &str = "tour.previous";
pub const NEXT_COMMAND: &str = "tour.next";
pub const OVERVIEW_COMMAND: &str = "tour.overview";
pub const THREAD_COMMAND: &str = "tour.backToThread";
pub const STOP_COMMAND: &str = "tour.stop";

/// Build the single-line control row for the active source file.
///
/// LSP code lenses carry one command each, so the row is a sequence of lenses
/// on the same line. Zed joins same-line lenses with ` | `, which renders
/// `◀ Prev | 2/N | Next ▶ | Overview | Stop`. The step counter is attached to
/// the Next lens so the row needs no non-interactive lens.
///
/// Boundary controls are hidden because they would be no-ops. The first step
/// renders `1/N | Next ▶ | Overview | Stop` and the last step renders
/// `◀ Prev | N/N | Overview | Stop`. At the last step the counter moves to
/// Overview, matching the single-step row.
///
/// A single-step tour omits Previous and Next because both would be no-ops.
/// The counter is attached to Overview instead.
///
/// A tour with a creating-thread link appends `Back to thread` before Stop. The
/// base row keeps the layout from #7; tours without a thread link are unchanged.
pub fn control_lenses(
    current_step: usize,
    step_count: usize,
    source_range: Range,
    source: &str,
    placement: Placement,
    has_thread_link: bool,
) -> Vec<CodeLens> {
    let line = control_line(source_range, source, placement);
    let range = Range::new(Position::new(line, 0), Position::new(line, 0));
    let counter = format!("{}/{}", current_step.saturating_add(1), step_count);

    let mut lenses = if step_count <= 1 {
        vec![code_lens(
            range,
            format!("{counter} | Overview"),
            OVERVIEW_COMMAND,
        )]
    } else {
        let at_first = current_step == 0;
        let at_last = current_step.saturating_add(1) >= step_count;
        let mut boundary_lenses = Vec::with_capacity(3);
        if !at_first {
            boundary_lenses.push(code_lens(range, "◀ Prev", PREVIOUS_COMMAND));
        }
        if !at_last {
            boundary_lenses.push(code_lens(
                range,
                format!("{counter} | Next ▶"),
                NEXT_COMMAND,
            ));
        }
        let overview_title = if at_last {
            format!("{counter} | Overview")
        } else {
            "Overview".to_owned()
        };
        boundary_lenses.push(code_lens(range, overview_title, OVERVIEW_COMMAND));
        boundary_lenses
    };
    if has_thread_link {
        lenses.push(code_lens(range, "Back to thread", THREAD_COMMAND));
    }
    lenses.push(code_lens(range, "Stop", STOP_COMMAND));
    lenses
}

/// Resolve the source line that anchors the control row.
///
/// Below placement uses the line after the selection so the row renders
/// between the selected code and the body hint. If the selection ends on the
/// last line, there is no line below, so the row falls back to the selection
/// start line. Above placement always uses the selection start line.
pub fn control_line(source_range: Range, source: &str, placement: Placement) -> u32 {
    match placement {
        Placement::Above => source_range.start.line,
        Placement::Below => {
            let next_line = source_range.end.line.saturating_add(1);
            if next_line < source_line_count(source) {
                next_line
            } else {
                source_range.start.line
            }
        }
    }
}

fn code_lens(range: Range, title: impl Into<String>, command: &str) -> CodeLens {
    CodeLens {
        range,
        command: Some(Command::new(title.into(), command.to_owned(), None)),
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use lsp_types::{CodeLens, Position, Range};

    use super::{
        NEXT_COMMAND, OVERVIEW_COMMAND, PREVIOUS_COMMAND, STOP_COMMAND, THREAD_COMMAND,
        control_lenses,
    };
    use crate::settings::Placement;

    fn range(start_line: u32, end_line: u32) -> Range {
        Range::new(Position::new(start_line, 0), Position::new(end_line, 4))
    }

    fn lens_rows(lenses: &[CodeLens]) -> (Vec<u32>, Vec<&str>, Vec<&str>, Vec<bool>) {
        let lines = lenses.iter().map(|lens| lens.range.start.line).collect();
        let titles = lenses
            .iter()
            .map(|lens| {
                lens.command
                    .as_ref()
                    .expect("control lens should have a command")
                    .title
                    .as_str()
            })
            .collect();
        let commands = lenses
            .iter()
            .map(|lens| {
                lens.command
                    .as_ref()
                    .expect("control lens should have a command")
                    .command
                    .as_str()
            })
            .collect();
        let without_arguments = lenses
            .iter()
            .map(|lens| {
                lens.command
                    .as_ref()
                    .expect("control lens should have a command")
                    .arguments
                    .is_none()
            })
            .collect();
        (lines, titles, commands, without_arguments)
    }

    #[test]
    fn below_placement_anchors_after_the_selection_and_wires_commands() {
        let lenses = control_lenses(
            1,
            3,
            range(1, 1),
            "line0\nline1\nline2\nline3\n",
            Placement::Below,
            false,
        );
        let (lines, titles, commands, without_arguments) = lens_rows(&lenses);

        assert_eq!(lines, vec![2, 2, 2, 2]);
        assert_eq!(titles, vec!["◀ Prev", "2/3 | Next ▶", "Overview", "Stop"]);
        assert_eq!(
            commands,
            vec![
                PREVIOUS_COMMAND,
                NEXT_COMMAND,
                OVERVIEW_COMMAND,
                STOP_COMMAND,
            ]
        );
        assert_eq!(without_arguments, vec![true, true, true, true]);
    }

    #[test]
    fn above_placement_anchors_at_the_selection_start_line() {
        let lenses = control_lenses(
            1,
            3,
            range(1, 2),
            "line0\nline1\nline2\nline3\n",
            Placement::Above,
            false,
        );
        let (lines, titles, commands, without_arguments) = lens_rows(&lenses);

        assert_eq!(lines, vec![1, 1, 1, 1]);
        assert_eq!(titles, vec!["◀ Prev", "2/3 | Next ▶", "Overview", "Stop"]);
        assert_eq!(
            commands,
            vec![
                PREVIOUS_COMMAND,
                NEXT_COMMAND,
                OVERVIEW_COMMAND,
                STOP_COMMAND,
            ]
        );
        assert_eq!(without_arguments, vec![true, true, true, true]);
    }

    #[test]
    fn last_line_fallback_anchors_at_the_selection_start_line() {
        let lenses = control_lenses(1, 3, range(1, 1), "line0\nline1", Placement::Below, false);
        let (lines, titles, commands, without_arguments) = lens_rows(&lenses);

        assert_eq!(lines, vec![1, 1, 1, 1]);
        assert_eq!(titles, vec!["◀ Prev", "2/3 | Next ▶", "Overview", "Stop"]);
        assert_eq!(
            commands,
            vec![
                PREVIOUS_COMMAND,
                NEXT_COMMAND,
                OVERVIEW_COMMAND,
                STOP_COMMAND,
            ]
        );
        assert_eq!(without_arguments, vec![true, true, true, true]);
    }

    #[test]
    fn trailing_newline_uses_the_empty_line_after_the_selection() {
        let lenses = control_lenses(0, 2, range(1, 1), "line0\nline1\n", Placement::Below, false);

        assert!(lenses.iter().all(|lens| lens.range.start.line == 2));
    }

    #[test]
    fn thread_link_appends_the_back_to_thread_control() {
        let lenses = control_lenses(1, 3, range(0, 0), "line0\nline1\n", Placement::Below, true);
        let (lines, titles, commands, without_arguments) = lens_rows(&lenses);

        assert_eq!(lines, vec![1, 1, 1, 1, 1]);
        assert_eq!(
            titles,
            vec![
                "◀ Prev",
                "2/3 | Next ▶",
                "Overview",
                "Back to thread",
                "Stop"
            ]
        );
        assert_eq!(
            commands,
            vec![
                PREVIOUS_COMMAND,
                NEXT_COMMAND,
                OVERVIEW_COMMAND,
                THREAD_COMMAND,
                STOP_COMMAND,
            ]
        );
        assert_eq!(without_arguments, vec![true, true, true, true, true]);
    }

    #[test]
    fn first_step_hides_previous_and_keeps_the_counter_on_next() {
        let lenses = control_lenses(0, 3, range(0, 0), "line0\nline1\n", Placement::Below, false);
        let (lines, titles, commands, without_arguments) = lens_rows(&lenses);

        assert_eq!(lines, vec![1, 1, 1]);
        assert_eq!(titles, vec!["1/3 | Next ▶", "Overview", "Stop"]);
        assert_eq!(commands, vec![NEXT_COMMAND, OVERVIEW_COMMAND, STOP_COMMAND]);
        assert_eq!(without_arguments, vec![true, true, true]);
    }

    #[test]
    fn last_step_hides_next_and_keeps_the_counter_on_overview() {
        let lenses = control_lenses(2, 3, range(0, 0), "line0\nline1\n", Placement::Below, false);
        let (lines, titles, commands, without_arguments) = lens_rows(&lenses);

        assert_eq!(lines, vec![1, 1, 1]);
        assert_eq!(titles, vec!["◀ Prev", "3/3 | Overview", "Stop"]);
        assert_eq!(
            commands,
            vec![PREVIOUS_COMMAND, OVERVIEW_COMMAND, STOP_COMMAND]
        );
        assert_eq!(without_arguments, vec![true, true, true]);
    }

    #[test]
    fn middle_step_keeps_both_boundary_controls() {
        let lenses = control_lenses(1, 3, range(0, 0), "line0\nline1\n", Placement::Below, false);
        let (lines, titles, commands, without_arguments) = lens_rows(&lenses);

        assert_eq!(lines, vec![1, 1, 1, 1]);
        assert_eq!(titles, vec!["◀ Prev", "2/3 | Next ▶", "Overview", "Stop"]);
        assert_eq!(
            commands,
            vec![
                PREVIOUS_COMMAND,
                NEXT_COMMAND,
                OVERVIEW_COMMAND,
                STOP_COMMAND,
            ]
        );
        assert_eq!(without_arguments, vec![true, true, true, true]);
    }

    #[test]
    fn boundary_rows_still_append_the_thread_control() {
        let first = control_lenses(0, 3, range(0, 0), "line0\nline1\n", Placement::Below, true);
        let (_, titles, _, _) = lens_rows(&first);
        assert_eq!(
            titles,
            vec!["1/3 | Next ▶", "Overview", "Back to thread", "Stop"]
        );

        let last = control_lenses(2, 3, range(0, 0), "line0\nline1\n", Placement::Below, true);
        let (_, titles, _, _) = lens_rows(&last);
        assert_eq!(
            titles,
            vec!["◀ Prev", "3/3 | Overview", "Back to thread", "Stop"]
        );
    }

    #[test]
    fn single_step_tours_omit_previous_and_next() {
        let lenses = control_lenses(0, 1, range(0, 0), "line0\nline1\n", Placement::Below, false);
        let (lines, titles, commands, without_arguments) = lens_rows(&lenses);

        assert_eq!(lines, vec![1, 1]);
        assert_eq!(titles, vec!["1/1 | Overview", "Stop"]);
        assert_eq!(commands, vec![OVERVIEW_COMMAND, STOP_COMMAND]);
        assert_eq!(without_arguments, vec![true, true]);
    }
}
