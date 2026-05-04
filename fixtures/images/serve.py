#!/usr/bin/env python3
"""serve.py — fixture server for runtimeImages detector (T75).

Routes:

  /clean/             — img loads, has alt + dims (control)
  /broken/            — img src=/404.png (404)
  /empty-src/         — img src=""
  /missing-alt/       — img with no alt attribute
  /cls-risk/          — img with no width/height + no aspect-ratio
  /combined/          — multiple offenders

Plus a 1×1 transparent PNG at /pixel.png so the clean fixture has a
reachable image.
"""
import argparse
import base64
import http.server
import socketserver
import sys
import time

# 1×1 transparent PNG.
PIXEL_PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNgAAAAAgABc3UBGAAAAABJRU5ErkJggg=="
)

PAGE = """<!doctype html>
<html lang=en><head>
<meta charset=utf-8>
<title>{title}</title>
<style>body{{margin:0;padding:1rem;font-family:system-ui,sans-serif}}</style>
</head><body>
<h1>{heading}</h1>
<p>Expected finding: <code>{expected}</code></p>
{body}
</body></html>
"""

ROUTES = {
    "/clean/": dict(
        title="clean", heading="Clean — image loads, alt + dims OK",
        expected="(no findings)",
        body='<img src="/pixel.png" alt="a 1px transparent dot" width="1" height="1">',
    ),
    "/broken/": dict(
        title="broken", heading="Broken — src=/404.png",
        expected="images.broken",
        body='<img src="/404.png" alt="missing image" width="64" height="64">',
    ),
    "/empty-src/": dict(
        title="empty-src", heading="Empty src",
        expected="images.empty-src",
        body='<img src="" alt="empty src" width="64" height="64">',
    ),
    "/missing-alt/": dict(
        title="missing-alt", heading="Missing alt attribute",
        expected="images.missing-alt-attr",
        body='<img src="/pixel.png" width="64" height="64">',
    ),
    "/cls-risk/": dict(
        title="cls-risk", heading="No width/height — CLS risk",
        expected="images.cls-risk",
        body='<img src="/pixel.png" alt="no dims" style="width:200px">',
    ),
    "/combined/": dict(
        title="combined", heading="Multiple",
        expected="multiple findings",
        body='<img src="/404.png" width="64" height="64"><br>'
             '<img src="/pixel.png" alt="a">',
    ),
}


class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):  # noqa: N802
        path = self.path.split("?", 1)[0]
        if path == "/":
            rows = "".join(
                f'<li><a href="{p}">{spec["heading"]}</a> — '
                f'expected: <code>{spec["expected"]}</code></li>'
                for p, spec in ROUTES.items()
            )
            self._send(200, "text/html", f"<ul>{rows}</ul>".encode())
        elif path in ROUTES:
            spec = ROUTES[path]
            body = PAGE.format(title=spec["title"], heading=spec["heading"],
                               expected=spec["expected"], body=spec["body"])
            self._send(200, "text/html; charset=utf-8", body.encode())
        elif path == "/pixel.png":
            self._send(200, "image/png", PIXEL_PNG)
        else:
            self._send(404, "text/html", b"<h1>404</h1>")

    def _send(self, status, ctype, body):
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def log_message(self, fmt, *args):
        ts = time.strftime("%H:%M:%S")
        sys.stdout.write(f"[img-fix {ts}] {self.address_string()} {fmt%args}\n")
        sys.stdout.flush()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8768)
    args = ap.parse_args()
    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer(("", args.port), Handler) as httpd:
        print(f"[img-fix] http://127.0.0.1:{args.port}/")
        httpd.serve_forever()


if __name__ == "__main__":
    main()
