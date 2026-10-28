use std::path::{Path, PathBuf};

use tour_lsp::tour::{TourPosition, parse_tour};

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

    for step in &tour.steps {
        assert!(
            step.target.raw_path.starts_with("../"),
            "step {} should use a path relative to examples/",
            step.number
        );
        assert!(
            step.target.path.exists(),
            "step {} target {} should exist",
            step.number,
            step.target.path.display()
        );
        assert!(
            !step.body_hint().is_empty(),
            "step {} should have a body hint",
            step.number
        );
    }

    assert_eq!(
        tour.steps[0].target.range.start,
        TourPosition {
            line: 66,
            column: 1
        }
    );
    assert_eq!(
        tour.steps[0].target.range.end,
        TourPosition {
            line: 66,
            column: 18
        }
    );
    assert_eq!(tour.steps[2].target.range.start.line, 235);
    assert_eq!(tour.steps[2].target.range.end.line, 249);
}
