"""Local stand-in for the nginx config: static files from website/, /api/ proxied to dctx-play, gzip off.
python devserver.py SITE_DIR PORT UPSTREAM_PORT"""
import http.client
import http.server
import sys
from pathlib import Path

SITE, PORT, UP = Path(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3])


class H(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k):
        super().__init__(*a, directory=str(SITE), **k)

    def log_message(self, *a):
        pass

    def proxy(self):
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0)) if self.command == "POST" else None
        try:
            c = http.client.HTTPConnection("127.0.0.1", UP, timeout=10)
            hdrs = {k: v for k, v in self.headers.items() if k.lower() in ("content-type", "content-length")}
            hdrs["X-Forwarded-For"] = self.client_address[0]
            c.request(self.command, self.path, body=body, headers=hdrs)
            r = c.getresponse()
            data = r.read()
            self.send_response(r.status)
            for k, v in r.getheaders():
                if k.lower() not in ("connection", "content-length"):
                    self.send_header(k, v)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        except OSError:
            page = (SITE / "playground/offline/index.html").read_bytes()
            self.send_response(503)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(page)))
            self.end_headers()
            self.wfile.write(page)

    def do_GET(self):
        if self.path.startswith("/api/"):
            return self.proxy()
        return super().do_GET()

    def do_POST(self):
        if self.path.startswith("/api/"):
            return self.proxy()
        self.send_error(405)

    def send_error(self, code, message=None, explain=None):
        if code == 404:
            page = (SITE / "404.html").read_bytes()
            self.send_response(404)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(page)))
            self.end_headers()
            self.wfile.write(page)
            return
        super().send_error(code, message, explain)


http.server.ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
