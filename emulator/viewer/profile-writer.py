#!/usr/bin/env python3
"""POST /profile updates color, input, and size. Width and height are optional."""

import json
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

CHOICES = Path(os.environ.get("OSEINK_PROFILE_CHOICES", "/usr/share/novnc/profile-choices.json"))
DEST = Path(os.environ.get("OSEINK_PROFILE_DEST", "/shared/run/profile.json"))
# shortcut: booted devd hashes share content, drop the stamp once that image hashes profile.json
STAMP = Path(os.environ.get("OSEINK_PROFILE_STAMP", "/shared/share/profile.stamp"))
HOST = os.environ.get("OSEINK_PROFILE_HOST", "127.0.0.1")
PORT = int(os.environ.get("OSEINK_PROFILE_PORT", "8765"))
MAX_EDGE = 4096


def canonical(doc):
    return json.dumps(doc, indent=2) + "\n"


def load_choices():
    return json.loads(CHOICES.read_text(encoding="utf-8"))


def encode_input(flags, table):
    if not isinstance(flags, list) or not flags or not all(isinstance(item, str) for item in flags):
        raise ValueError
    want = set(flags)
    if len(want) != len(flags):
        raise ValueError
    for name, parts in table.items():
        if set(parts) == want:
            return name
    raise ValueError


def panel_edge(body, key, previous):
    if key not in body:
        return previous
    val = body[key]
    if isinstance(val, bool) or not isinstance(val, int) or val < 1 or val > MAX_EDGE:
        raise ValueError
    return val


def apply_selection(doc, body, choices):
    mode = body["color"]
    # shortcut: color writes kaleido, keep full when the file already says full, until the picker splits those
    if mode == "bw":
        color = choices["bw"]
    elif mode == "color":
        color = "full" if doc.get("color") == "full" else choices["color_on"]
    else:
        raise ValueError
    updated = dict(doc)
    updated["color"] = color
    updated["input"] = encode_input(body["input"], choices["input"])
    updated["width"] = panel_edge(body, "width", doc["width"])
    updated["height"] = panel_edge(body, "height", doc["height"])
    return updated


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        if self.path != "/profile":
            self._send(404, b"")
            return
        try:
            data = DEST.read_bytes()
        except OSError:
            self._send(404, b"")
            return
        self._send(200, data, "application/json")

    def do_POST(self):
        if self.path != "/profile":
            self._send(404, b"")
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            self._send(400, b"")
            return
        if length < 0 or length > 256:
            self._send(400, b"")
            return
        try:
            body = json.loads(self.rfile.read(length) or b"{}")
            doc = json.loads(DEST.read_text(encoding="utf-8"))
            updated = apply_selection(doc, body, load_choices())
        except (OSError, json.JSONDecodeError, KeyError, TypeError, ValueError, AttributeError):
            self._send(400, b"")
            return
        text = canonical(updated)
        tmp = DEST.with_suffix(".json.tmp")
        tmp.write_text(text, encoding="utf-8")
        tmp.replace(DEST)
        DEST.chmod(0o666)
        STAMP.parent.mkdir(parents=True, exist_ok=True)
        STAMP.write_text(text, encoding="utf-8")
        STAMP.chmod(0o666)
        self._send(200, b'{"ok":true}\n', "application/json")

    def _send(self, code, data, content_type="text/plain"):
        self.send_response(code)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, fmt, *args):
        return


def main():
    ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()


if __name__ == "__main__":
    main()
