# Repro fixture for docs/debug/2026-11-18-windows-update-hang.md (DL-2): local bucket server on
# 127.0.0.1:18765 serving manifest.json; returns 503 while a file named `fail` exists next to it.
# Copied from gitignored scratch docs/tmp/claude-scratch/dl2/ on 2026-11-20.
import http.server, os, sys
D=os.path.dirname(os.path.abspath(__file__))
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if os.path.exists(D+'/fail'):
            self.send_response(503); self.send_header('Content-Length','0'); self.end_headers(); return
        b=open(D+'/manifest.json','rb').read()
        self.send_response(200); self.send_header('Content-Length',str(len(b))); self.end_headers(); self.wfile.write(b)
    def log_message(self,*a): sys.stderr.write("REQ %s\n"%(a[1],))
http.server.HTTPServer(('127.0.0.1',18765),H).serve_forever()
