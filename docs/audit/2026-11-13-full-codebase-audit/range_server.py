# Repro fixture for docs/audit/2026-11-13-full-codebase-audit.md (Range/ETag HTTP server).
# Copied from gitignored scratch docs/tmp/claude-scratch/range_server.py on 2026-11-20.
"""Audit scratch: HTTP server with Range + ETag support, logs every request's Range header.
Usage: python3 range_server.py <file> <port> [checksum_probe_status]"""
import sys, re
from http.server import BaseHTTPRequestHandler, HTTPServer

DATA = open(sys.argv[1], "rb").read()
ETAG = '"audit-v1"'
# Optional 3rd arg: status code returned for checksum-file probes (e.g. 503)
STATUS = int(sys.argv[3]) if len(sys.argv) > 3 else 0

class H(BaseHTTPRequestHandler):
    def do_GET(self):
        rng = self.headers.get("Range")
        sys.stderr.write(f"REQ {self.path} Range={rng}\n")
        if STATUS and self.path.endswith((".sha256", "checksums.txt", "SHA256SUMS")):
            self.send_response(STATUS); self.send_header("Content-Length", "0"); self.end_headers(); return
        m = re.match(r"bytes=(\d+)-(\d*)", rng or "")
        if m:
            s = int(m.group(1)); e = int(m.group(2)) if m.group(2) else len(DATA) - 1
            body = DATA[s:e + 1]
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {s}-{e}/{len(DATA)}")
        else:
            body = DATA
            self.send_response(200)
        self.send_header("ETag", ETAG)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass
    def log_message(self, *a):
        pass

HTTPServer(("127.0.0.1", int(sys.argv[2])), H).serve_forever()
