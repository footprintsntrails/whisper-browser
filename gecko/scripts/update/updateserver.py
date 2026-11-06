#!/usr/bin/env python3
"""A local stand-in for Whisper's update host, for the update test.

Serves the files of ROOT on 127.0.0.1:PORT the way the real hosts do: update.xml as
GitHub Pages would, and MARs the way a GitHub release does, where
/download/<name> answers with a redirect to /files/<name> (github.com redirects to
release-assets.githubusercontent.com). Byte ranges are honoured, as the browser
downloads updates with range requests. Every request is written to LOG as one JSON line.

Usage: updateserver.py --root DIR --port 8111 --log requests.jsonl
"""

import argparse
import json
import os
import posixpath
import re
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import unquote, urlsplit


class Handler(BaseHTTPRequestHandler):
    root = None
    log_file = None
    lock = threading.Lock()

    def log_message(self, fmt, *args):  # the JSON log is enough
        pass

    def record(self, status, sent=0, extra=None):
        entry = {"t": round(time.time(), 3), "method": self.command, "path": self.path, "status": status,
                 "range": self.headers.get("Range"), "bytes": sent, "agent": self.headers.get("User-Agent")}
        entry.update(extra or {})
        with self.lock:
            self.log_file.write(json.dumps(entry) + "\n")
            self.log_file.flush()

    def do_HEAD(self):
        self.serve(head=True)

    def do_GET(self):
        self.serve(head=False)

    def serve(self, head):
        path = posixpath.normpath(unquote(urlsplit(self.path).path))
        if path.startswith("/download/"):
            target = "/files/" + path[len("/download/"):]
            self.send_response(302)
            self.send_header("Location", target)
            self.send_header("Content-Length", "0")
            self.end_headers()
            self.record(302, extra={"location": target})
            return
        full = os.path.join(self.root, path.lstrip("/"))
        if not os.path.realpath(full).startswith(os.path.realpath(self.root) + os.sep) or not os.path.isfile(full):
            self.send_error(404)
            self.record(404)
            return
        size = os.path.getsize(full)
        start, end, status = 0, size - 1, 200
        m = re.fullmatch(r"bytes=(\d+)-(\d*)", self.headers.get("Range", "") or "")
        if m:
            start = int(m.group(1))
            end = min(int(m.group(2)), size - 1) if m.group(2) else size - 1
            if start >= size:
                self.send_response(416)
                self.send_header("Content-Range", f"bytes */{size}")
                self.send_header("Content-Length", "0")
                self.end_headers()
                self.record(416)
                return
            status = 206
        length = end - start + 1
        self.send_response(status)
        ctype = "text/xml" if full.endswith(".xml") else "application/octet-stream"
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(length))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Last-Modified", self.date_time_string(int(os.path.getmtime(full))))
        if status == 206:
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.end_headers()
        sent = 0
        if not head:
            with open(full, "rb") as f:
                f.seek(start)
                while sent < length:
                    chunk = f.read(min(1 << 16, length - sent))
                    if not chunk:
                        break
                    try:
                        self.wfile.write(chunk)
                    except (BrokenPipeError, ConnectionResetError):
                        break
                    sent += len(chunk)
        self.record(status, sent)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", required=True)
    ap.add_argument("--port", type=int, default=8111)
    ap.add_argument("--log", required=True)
    a = ap.parse_args()
    Handler.root = os.path.realpath(a.root)
    Handler.log_file = open(a.log, "a", buffering=1)
    server = ThreadingHTTPServer(("127.0.0.1", a.port), Handler)
    server.daemon_threads = True
    print(f"update server: {Handler.root} on http://127.0.0.1:{a.port}/", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
