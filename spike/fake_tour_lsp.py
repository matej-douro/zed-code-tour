#!/usr/bin/env python3
"""Throwaway code tour layout LSP for issue #1.

This server is spike-only. It is not imported by the extension or the real
language server. It returns one code lens and one or two inlay hints so the
layout can be checked by hand or by the tests in this directory.

The hint and lens positions are computed by layout.py. The server omits
`kind`, `paddingRight`, and `tooltip`. It can send the standard `paddingLeft`
flag, which is not a style but also gives the hint a right-biased anchor.
"""

from __future__ import annotations

import argparse
import json
import sys

sys.dont_write_bytecode = True

from layout import (
    LayoutCase,
    Selection,
    case_by_name,
    code_lenses,
    compute_layout,
    display_rows,
    inlay_hints,
    line_count,
)


def read_message(stream):
    headers = {}
    while True:
        line = stream.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, separator, value = line.decode("ascii").partition(":")
        if not separator:
            raise ValueError(f"invalid header: {line!r}")
        headers[name.strip().lower()] = value.strip()

    length = int(headers["content-length"])
    body = stream.read(length)
    if len(body) != length:
        raise ValueError("truncated message body")
    return json.loads(body)


def write_message(message):
    body = json.dumps(message, separators=(",", ":"), ensure_ascii=False).encode(
        "utf-8"
    )
    header = f"Content-Length: {len(body)}\r\n\r\n".encode("ascii")
    sys.stdout.buffer.write(header)
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()


def response(request_id, result=None):
    return {"jsonrpc": "2.0", "id": request_id, "result": result}


def error(request_id, code, message):
    return {
        "jsonrpc": "2.0",
        "id": request_id,
        "error": {"code": code, "message": message},
    }


def build_case(args) -> LayoutCase:
    if args.case:
        if args.source:
            raise SystemExit("use either --case or --source, not both")
        return case_by_name(args.case)

    if not args.source:
        raise SystemExit("--case or --source is required")
    if None in (
        args.start_line,
        args.start_character,
        args.end_line,
        args.end_character,
    ):
        raise SystemExit(
            "--source requires --start-line, --start-character, "
            "--end-line, and --end-character"
        )

    with open(args.source, encoding="utf-8") as source_file:
        source = source_file.read()

    selection = Selection(
        start_line=args.start_line,
        start_character=args.start_character,
        end_line=args.end_line,
        end_character=args.end_character,
    )
    if selection.end_line >= line_count(source):
        raise SystemExit("selection end line is outside the source file")

    return LayoutCase(
        name=f"source:{args.source}",
        source=source,
        selection=selection,
        placement=args.placement,
        padding_left=args.padding_left,
        two_hints=args.two_hints,
        force_fallback=args.force_fallback,
    )


def dump(case: LayoutCase):
    layout = compute_layout(case)
    payload = {
        "case": case.name,
        "placement": case.placement,
        "selection": {
            "start": {
                "line": case.selection.start_line,
                "character": case.selection.start_character,
            },
            "end": {
                "line": case.selection.end_line,
                "character": case.selection.end_character,
            },
        },
        "line_count": line_count(case.source),
        "fallback": layout.fallback,
        "next_line_exists": layout.next_line_exists,
        "hint_bias": layout.hint_bias,
        "padding_left": case.padding_left,
        "hint_position": layout.hint_position,
        "hint_text": layout.hint_text,
        "lens_line": layout.lens_line,
        "inlay_hints": inlay_hints(case),
        "code_lenses": code_lenses(case),
        "rows_hints_and_lens_on": display_rows(case),
        "rows_hints_on_lens_off": display_rows(
            case, code_lens_enabled=False
        ),
        "rows_hints_off_lens_on": display_rows(
            case, inlay_hints_enabled=False
        ),
        "rows_hints_and_lens_off": display_rows(
            case, inlay_hints_enabled=False, code_lens_enabled=False
        ),
    }
    json.dump(payload, sys.stdout, indent=2, ensure_ascii=False)
    sys.stdout.write("\n")


def serve(case: LayoutCase):
    while True:
        message = read_message(sys.stdin.buffer)
        if message is None:
            return

        method = message.get("method")
        request_id = message.get("id")

        if method == "initialize":
            write_message(
                response(
                    request_id,
                    {
                        "capabilities": {
                            "textDocumentSync": 1,
                            "inlayHintProvider": True,
                            "codeLensProvider": {"resolveProvider": False},
                        },
                        "serverInfo": {
                            "name": "tour-layout-spike",
                            "version": "0.0.0",
                        },
                    },
                )
            )
        elif method == "initialized":
            continue
        elif method == "shutdown":
            write_message(response(request_id))
        elif method == "exit":
            return
        elif method == "textDocument/inlayHint":
            write_message(response(request_id, inlay_hints(case)))
        elif method == "textDocument/codeLens":
            write_message(response(request_id, code_lenses(case)))
        elif method in (
            "textDocument/didOpen",
            "textDocument/didChange",
            "textDocument/didSave",
            "textDocument/didClose",
        ):
            continue
        elif request_id is not None:
            write_message(error(request_id, -32601, f"unhandled method: {method}"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case", help="named layout case from layout.py")
    parser.add_argument("--source", help="source file to read")
    parser.add_argument("--start-line", type=int)
    parser.add_argument("--start-character", type=int)
    parser.add_argument("--end-line", type=int)
    parser.add_argument("--end-character", type=int)
    parser.add_argument(
        "--placement", choices=("below", "above"), default="below"
    )
    parser.add_argument(
        "--padding-left",
        action="store_true",
        help="send paddingLeft and a leading space to force a right-biased hint",
    )
    parser.add_argument("--two-hints", action="store_true")
    parser.add_argument(
        "--force-fallback",
        action="store_true",
        help="force last-line end anchoring even when a line follows",
    )
    parser.add_argument(
        "--dump",
        action="store_true",
        help="print layout payloads as JSON instead of serving LSP",
    )
    args = parser.parse_args()

    case = build_case(args)
    if args.dump:
        dump(case)
    else:
        serve(case)


if __name__ == "__main__":
    main()
