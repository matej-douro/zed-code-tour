use std::path::{Path, PathBuf};

use tour_lsp::tour::parse_tour;

const SAMPLE_TOUR: &str = "examples/zed-code-tour.tour.md";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root should resolve")
}

#[test]
fn sample_tour_parses_and_targets_real_files() {
    let tour_path = repo_root().join(SAMPLE_TOUR);
    let source = std::fs::read_to_string(&tour_path).expect("sample tour should be readable");

    let tour = parse_tour(&tour_path, &source).expect("sample tour should parse");

    assert_eq!(tour.title, "Inside zed-code-tour");
    assert_eq!(
        tour.thread_link.as_ref().map(|link| link.uri.as_str()),
        Some("zed:///agent/thread/sample-session?name=Code%20tour%20example")
    );
    assert_eq!(tour.steps.len(), 3);

    let expected_starts = [
        (66, "fn parse_tour"),
        (5, "pub struct Tour"),
        (439, "fn code_lenses_for"),
    ];

    for (step, (start_line, token)) in tour.steps.iter().zip(expected_starts) {
        assert!(
            step.target.raw_path.starts_with("../"),
            "step {} should use a path relative to examples/",
            step.number
        );

        let target_source = std::fs::read_to_string(&step.target.path)
            .unwrap_or_else(|_| panic!("step {} target should exist", step.number));
        let target_lines: Vec<_> = target_source.lines().collect();
        let range = step.target.range;

        assert_eq!(
            range.start.line, start_line,
            "step {} should start on the expected line",
            step.number
        );
        assert!(
            range.end.line as usize <= target_lines.len(),
            "step {} range ends past the end of {}",
            step.number,
            step.target.path.display()
        );

        let start_text = target_lines[range.start.line as usize - 1];
        assert!(
            start_text.contains(token),
            "step {} should start on a line containing `{token}`, got `{start_text}`",
            step.number
        );
        assert!(
            range.start.column as usize <= start_text.chars().count() + 1,
            "step {} start column is past the end of the line",
            step.number
        );

        let end_text = target_lines[range.end.line as usize - 1];
        assert!(
            range.end.column as usize <= end_text.chars().count() + 1,
            "step {} end column is past the end of the line",
            step.number
        );

        assert!(
            !step.body_hint().is_empty(),
            "step {} should have a body hint",
            step.number
        );
    }
}
