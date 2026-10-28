from __future__ import annotations

import json
import os
import select
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from layout import (
    DEFAULT_BODY,
    LEFT_RULE,
    LENS_TEXT,
    Selection,
    body_text,
    case_by_name,
    code_lenses,
    compute_layout,
    display_rows,
    inlay_hints,
    line_count,
    line_length,
)

HERE = Path(__file__).resolve().parent
SERVER = HERE / "fake_tour_lsp.py"


class LayoutTests(unittest.TestCase):
    def test_below_middle_anchors_at_line_after_selection(self):
        case = case_by_name("below_middle")
        layout = compute_layout(case)
        self.assertFalse(layout.fallback)
        self.assertEqual(layout.hint_position, {"line": 3, "character": 0})
        self.assertEqual(layout.lens_line, 3)
        self.assertEqual(layout.hint_bias, "right")
        self.assertEqual(
            layout.hint_text,
            body_text(DEFAULT_BODY, padding_left=True),
        )
        self.assertTrue(inlay_hints(case)[0]["paddingLeft"])

        body = body_text(DEFAULT_BODY, padding_left=True).splitlines()
        rows = display_rows(case)
        self.assertEqual(
            rows,
            [
                "0: line0",
                "1: line1",
                "2: line2",
                f"lens: {LENS_TEXT}",
                f"hint: {body[0]}",
                f"hint: {body[1]}",
                "3: line3",
                "4: line4",
            ],
        )

    def test_below_middle_without_padding_puts_lens_after_body(self):
        case = case_by_name("below_middle_no_padding")
        layout = compute_layout(case)
        self.assertEqual(layout.hint_bias, "left")
        self.assertNotIn("paddingLeft", inlay_hints(case)[0])

        body = body_text(DEFAULT_BODY).splitlines()
        self.assertEqual(
            display_rows(case),
            [
                "0: line0",
                "1: line1",
                "2: line2",
                f"hint: {body[0]}",
                f"hint: {body[1]}",
                f"lens: {LENS_TEXT}",
                "3: line3",
                "4: line4",
            ],
        )

    def test_above_middle_puts_lens_before_body_then_selection(self):
        case = case_by_name("above_middle")
        layout = compute_layout(case)
        self.assertFalse(layout.fallback)
        self.assertEqual(layout.hint_position, {"line": 2, "character": 0})
        self.assertEqual(layout.lens_line, 2)
        self.assertEqual(layout.hint_bias, "right")
        self.assertFalse(layout.hint_text.startswith("\n"))

        body = body_text(DEFAULT_BODY, padding_left=True).splitlines()
        rows = display_rows(case)
        self.assertEqual(
            rows,
            [
                "0: line0",
                "1: line1",
                f"lens: {LENS_TEXT}",
                f"hint: {body[0]}",
                f"hint: {body[1]}",
                "2: line2",
                "3: line3",
                "4: line4",
            ],
        )

    def test_above_middle_without_padding_puts_lens_between_body_and_selection(self):
        case = case_by_name("above_middle_no_padding")
        layout = compute_layout(case)
        self.assertEqual(layout.hint_bias, "left")

        body = body_text(DEFAULT_BODY).splitlines()
        self.assertEqual(
            display_rows(case),
            [
                "0: line0",
                "1: line1",
                f"hint: {body[0]}",
                f"hint: {body[1]}",
                f"lens: {LENS_TEXT}",
                "2: line2",
                "3: line3",
                "4: line4",
            ],
        )

    def test_last_line_without_trailing_newline_uses_fallback(self):
        case = case_by_name("below_last_no_trailing_newline")
        layout = compute_layout(case)
        self.assertTrue(layout.fallback)
        self.assertFalse(layout.next_line_exists)
        self.assertEqual(layout.hint_position, {"line": 2, "character": 5})
        self.assertEqual(layout.lens_line, 2)
        self.assertEqual(layout.hint_bias, "right")
        self.assertTrue(layout.hint_text.startswith("\n"))

        body = body_text(DEFAULT_BODY).splitlines()
        rows = display_rows(case)
        self.assertEqual(
            rows,
            [
                "0: line0",
                "1: line1",
                f"lens: {LENS_TEXT}",
                "2: line2",
                f"hint: {body[0]}",
                f"hint: {body[1]}",
            ],
        )

    def test_last_content_line_with_trailing_newline_normal_uses_empty_line(self):
        case = case_by_name("below_last_with_trailing_newline_normal")
        layout = compute_layout(case)
        self.assertFalse(layout.fallback)
        self.assertTrue(layout.next_line_exists)
        self.assertEqual(layout.hint_position, {"line": 3, "character": 0})
        self.assertEqual(layout.lens_line, 3)
        self.assertEqual(layout.hint_bias, "right")

        body = body_text(DEFAULT_BODY, padding_left=True).splitlines()
        rows = display_rows(case)
        self.assertEqual(
            rows,
            [
                "0: line0",
                "1: line1",
                "2: line2",
                f"lens: {LENS_TEXT}",
                f"hint: {body[0]}",
                f"hint: {body[1]}",
                "3: ",
            ],
        )

    def test_last_content_line_with_trailing_newline_forced_fallback(self):
        case = case_by_name("below_last_with_trailing_newline_forced_fallback")
        layout = compute_layout(case)
        self.assertTrue(layout.fallback)
        self.assertTrue(layout.next_line_exists)
        self.assertEqual(layout.hint_position, {"line": 2, "character": 5})
        self.assertEqual(layout.lens_line, 2)

        body = body_text(DEFAULT_BODY).splitlines()
        rows = display_rows(case)
        self.assertEqual(
            rows,
            [
                "0: line0",
                "1: line1",
                f"lens: {LENS_TEXT}",
                "2: line2",
                f"hint: {body[0]}",
                f"hint: {body[1]}",
                "3: ",
            ],
        )

    def test_two_hints_have_no_per_hint_style_fields(self):
        case = case_by_name("below_middle_two_hints")
        hints = inlay_hints(case)
        self.assertEqual(len(hints), 2)
        for hint in hints:
            self.assertEqual(
                set(hint), {"position", "label", "paddingLeft"}
            )
            self.assertTrue(hint["paddingLeft"])
            self.assertNotIn("kind", hint)
            self.assertNotIn("paddingRight", hint)
            self.assertNotIn("tooltip", hint)

    def test_disabled_settings_remove_only_their_layer(self):
        case = case_by_name("below_middle")
        source_rows = [f"{index}: {line}" for index, line in enumerate(case.source.split("\n"))]

        both_off = display_rows(
            case, inlay_hints_enabled=False, code_lens_enabled=False
        )
        self.assertEqual(both_off, source_rows)

        hints_only = display_rows(case, code_lens_enabled=False)
        self.assertEqual(len(hints_only), len(source_rows) + 2)
        self.assertFalse(any(row.startswith("lens: ") for row in hints_only))

        lens_only = display_rows(case, inlay_hints_enabled=False)
        self.assertEqual(len(lens_only), len(source_rows) + 1)
        self.assertFalse(any(row.startswith("hint: ") for row in lens_only))
        self.assertIn(f"lens: {LENS_TEXT}", lens_only)

    def test_body_lines_share_padding_and_end_with_newline(self):
        text = body_text(DEFAULT_BODY)
        self.assertTrue(text.endswith("\n"))
        lines = text.split("\n")[:-1]
        self.assertTrue(all(line.startswith(f"{LEFT_RULE} ") for line in lines))
        self.assertEqual(len({len(line) for line in lines}), 1)

        padded = body_text(DEFAULT_BODY, padding_left=True)
        padded_lines = padded.split("\n")[:-1]
        self.assertTrue(
            all(line.startswith(f" {LEFT_RULE} ") for line in padded_lines)
        )
        self.assertEqual(len({len(line) for line in padded_lines}), 1)

    def test_lsp_line_accounting(self):
        self.assertEqual(line_count("a"), 1)
        self.assertEqual(line_count("a\n"), 2)
        self.assertEqual(line_count("a\n\n"), 3)
        self.assertEqual(line_length("a\n", 0), 1)
        self.assertEqual(line_length("a\n", 1), 0)

    def test_code_lens_anchor_uses_zero_width_line_start_range(self):
        lens = code_lenses(case_by_name("below_middle"))[0]
        self.assertEqual(lens["range"]["start"]["character"], 0)
        self.assertEqual(lens["range"]["start"], lens["range"]["end"])
        self.assertEqual(lens["range"]["start"]["line"], 3)


def write_message(process, message):
    body = json.dumps(message).encode("utf-8")
    process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode("ascii"))
    process.stdin.write(body)
    process.stdin.flush()


def read_message(process, timeout=10):
    ready, _, _ = select.select([process.stdout], [], [], timeout)
    if not ready:
        raise TimeoutError("timed out waiting for LSP response")

    headers = {}
    while True:
        line = process.stdout.readline()
        if not line:
            raise RuntimeError("LSP server closed stdout")
        if line in (b"\r\n", b"\n"):
            break
        name, _, value = line.decode("ascii").partition(":")
        headers[name.strip().lower()] = value.strip()

    length = int(headers["content-length"])
    body = process.stdout.read(length)
    return json.loads(body)


class ServerTests(unittest.TestCase):
    def start_server(self, *args):
        process = subprocess.Popen(
            [sys.executable, "-B", str(SERVER), *args],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        self.addCleanup(self.stop_server, process)
        return process

    def stop_server(self, process):
        try:
            if process.poll() is None:
                try:
                    write_message(process, {"jsonrpc": "2.0", "method": "exit"})
                except (BrokenPipeError, ValueError):
                    pass
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
        finally:
            for stream in (process.stdin, process.stdout, process.stderr):
                if stream is not None:
                    stream.close()

    def initialize(self, process):
        write_message(
            process,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {"capabilities": {}},
            },
        )
        initialized = read_message(process)
        self.assertEqual(initialized["id"], 1)
        capabilities = initialized["result"]["capabilities"]
        self.assertTrue(capabilities["inlayHintProvider"])
        self.assertEqual(
            capabilities["codeLensProvider"], {"resolveProvider": False}
        )
        write_message(
            process,
            {"jsonrpc": "2.0", "method": "initialized", "params": {}},
        )

    def test_server_returns_below_middle_payloads(self):
        process = self.start_server("--case", "below_middle")
        self.initialize(process)

        write_message(
            process,
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/codeLens",
                "params": {},
            },
        )
        lens_response = read_message(process)
        lens = lens_response["result"][0]
        self.assertEqual(lens["range"]["start"], {"line": 3, "character": 0})
        self.assertEqual(lens["range"]["end"], {"line": 3, "character": 0})

        write_message(
            process,
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "textDocument/inlayHint",
                "params": {},
            },
        )
        hint_response = read_message(process)
        hint = hint_response["result"][0]
        self.assertEqual(hint["position"], {"line": 3, "character": 0})
        self.assertEqual(set(hint), {"position", "label", "paddingLeft"})
        self.assertTrue(hint["paddingLeft"])
        self.assertTrue(hint["label"].startswith(f" {LEFT_RULE} "))

    def test_server_reads_source_file_for_last_line_fallback(self):
        with tempfile.NamedTemporaryFile(
            "w", suffix=".py", delete=False, encoding="utf-8"
        ) as source_file:
            source_file.write("line0\nline1\nline2")
            source_path = source_file.name
        self.addCleanup(os.unlink, source_path)

        process = self.start_server(
            "--source",
            source_path,
            "--start-line",
            "2",
            "--start-character",
            "0",
            "--end-line",
            "2",
            "--end-character",
            "5",
        )
        self.initialize(process)

        write_message(
            process,
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/inlayHint",
                "params": {},
            },
        )
        hint = read_message(process)["result"][0]
        self.assertEqual(hint["position"], {"line": 2, "character": 5})
        self.assertTrue(hint["label"].startswith("\n"))

    def test_server_survives_unknown_notification(self):
        process = self.start_server("--case", "above_middle")
        self.initialize(process)
        write_message(
            process,
            {
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {},
            },
        )
        write_message(
            process,
            {
                "jsonrpc": "2.0",
                "id": 9,
                "method": "textDocument/inlayHint",
                "params": {},
            },
        )
        hint = read_message(process)["result"][0]
        self.assertEqual(hint["position"], {"line": 2, "character": 0})


if __name__ == "__main__":
    unittest.main()
