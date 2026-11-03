mod support;

use std::path::{Path, PathBuf};

use serde_json::json;
use tour_lsp::tour::parse_tour;
use tour_lsp::uri::file_uri;

use support::{LspClient, lens_titles, request_methods, request_params, result};

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
        (68, "fn parse_tour"),
        (5, "pub struct Tour"),
        (511, "fn code_lenses_for"),
    ];

    for (step, (start_line, token)) in tour.steps.iter().zip(expected_starts) {
        assert!(
            step.target.raw_path.starts_with("../"),
            "step {} should use a path relative to examples/",
            step.number
        );
        assert!(
            !step.target.path.to_string_lossy().contains(".."),
            "step {} target path should be normalized",
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

#[test]
fn sample_tour_works_end_to_end_with_normalized_source_uris() {
    let tour_path = repo_root().join(SAMPLE_TOUR);
    let tour_source = std::fs::read_to_string(&tour_path).expect("sample tour should be readable");
    let tour = parse_tour(&tour_path, &tour_source).expect("sample tour should parse");
    let tour_uri = file_uri(&tour_path).expect("tour URI");

    let mut client = LspClient::start();
    client.initialize_with_options(Some(json!({
        "placement": "below",
        "auto_start": "never",
    })));

    let mut source_uris = Vec::new();
    for step in &tour.steps {
        let uri = file_uri(&step.target.path).expect("source URI");
        let text = std::fs::read_to_string(&step.target.path)
            .unwrap_or_else(|_| panic!("step {} target should exist", step.number));
        client.open_source(uri.as_str(), &text);
        source_uris.push(uri);
    }

    client.open_tour(tour_uri.as_str(), &tour_source);
    let messages = client.receive_exact(2);
    assert_eq!(
        request_methods(&messages),
        vec!["workspace/inlayHint/refresh", "workspace/codeLens/refresh"]
    );

    for (index, uri) in source_uris.iter().enumerate() {
        assert!(
            !uri.as_str().contains(".."),
            "source URI should be normalized: {}",
            uri.as_str()
        );

        let (messages, response) = if index == 0 {
            client.execute_command("tour.start", Vec::new())
        } else {
            client.execute_command("tour.next", Vec::new())
        };
        assert_eq!(result(&response), &json!(null));
        let show_document =
            request_params(&messages, "window/showDocument").expect("missing showDocument");
        assert_eq!(show_document["uri"], json!(uri.as_str()));
        assert_eq!(
            request_methods(&messages),
            vec!["window/showDocument", "workspace/codeLens/refresh"]
        );
        assert_eq!(
            client.receive()["method"],
            json!("workspace/inlayHint/refresh"),
            "the step change should refresh hints after the client scrolls"
        );

        let (messages, response) = client.code_lenses(uri.as_str());
        assert!(messages.is_empty());
        let counter = format!("{}/{}", index + 1, source_uris.len());
        let expected_row = if index == 0 {
            format!("{counter} | Next ▶ | Overview | Back to thread | Stop")
        } else if index + 1 == source_uris.len() {
            format!("◀ Prev | {counter} | Overview | Back to thread | Stop")
        } else {
            format!("◀ Prev | {counter} | Next ▶ | Overview | Back to thread | Stop")
        };
        assert_eq!(lens_titles(&response).join(" | "), expected_row);

        let (messages, response) = client.inlay_hints(uri.as_str());
        assert!(messages.is_empty());
        assert_eq!(
            result(&response)
                .as_array()
                .expect("inlay hint array")
                .len(),
            1
        );
    }

    client.finish();
}
