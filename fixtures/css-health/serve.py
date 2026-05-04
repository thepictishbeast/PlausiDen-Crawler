#!/usr/bin/env python3
"""serve.py — fixture server for the cssHealth detector.

Each route is engineered to trigger ONE specific finding kind. The
crawler runs against this server in CI to prove every detection arm
fires when it should AND stays silent when it should.

Routes (path → expected finding kind):

  /working/            — none (control)
  /missing/            — css.all-sheets-failed-network
  /partial/            — css.some-sheets-failed-network
  /empty/              — css.empty-or-tiny-body
  /wrongmime/          — css.wrong-mime
  /parsefail/          — css.served-but-not-applied (early bracket bug
                          drops most rules)
  /no-stylesheets/     — css.no-stylesheets-declared (warn)

The server intentionally lies on a few endpoints (404, 0-byte, wrong
content-type) — that's the bug we're trying to detect.
"""
import argparse
import http.server
import socketserver
import sys
import time

WORKING_CSS = b"""
html, body { background: #0f172a; color: #f8fafc; margin: 0; }
body { font-family: 'Inter', system-ui, sans-serif; padding: 2rem; }
h1 { color: #38bdf8; }
""".strip()

# Intentionally invalid CSS: opening brace never closes, so the
# browser's parser drops the entire sheet after this rule.
PARSEFAIL_CSS = b"""
body {
  background: #0f172a;
  color: #f8fafc
  /* missing semicolon AND missing closing brace */
} } } } }
@import url(does-not-exist);
@@@@@invalid@@@@@
""".strip()

PAGE_TEMPLATE = """<!doctype html>
<html lang=en><head>
<meta charset=utf-8>
<title>{title}</title>
{css_links}
</head><body>
<h1>{heading}</h1>
<p>This fixture should trigger: <code>{expected_finding}</code></p>
<p>If you see this page styled with a dark blue background, CSS applied.</p>
<p>If you see this page with a white background and serif font, no CSS applied.</p>
</body></html>
"""

ROUTES = {
    "/working/": {
        "title": "working CSS",
        "heading": "Working CSS — control",
        "expected": "none (control case)",
        "css_links": ['<link rel="stylesheet" href="/working.css">'],
    },
    "/missing/": {
        "title": "missing CSS",
        "heading": "Missing CSS — link points to 404",
        "expected": "css.all-sheets-failed-network",
        "css_links": ['<link rel="stylesheet" href="/does-not-exist.css">'],
    },
    "/partial/": {
        "title": "partial CSS",
        "heading": "Partial CSS — one OK, one 404",
        "expected": "css.some-sheets-failed-network",
        "css_links": [
            '<link rel="stylesheet" href="/working.css">',
            '<link rel="stylesheet" href="/does-not-exist.css">',
        ],
    },
    "/empty/": {
        "title": "empty CSS",
        "heading": "Empty CSS — link returns 0 bytes",
        "expected": "css.empty-or-tiny-body",
        "css_links": ['<link rel="stylesheet" href="/empty.css">'],
    },
    "/wrongmime/": {
        "title": "wrong-MIME CSS",
        "heading": "Wrong MIME — server claims text/html",
        "expected": "css.wrong-mime",
        "css_links": ['<link rel="stylesheet" href="/wrongmime.css">'],
    },
    "/parsefail/": {
        "title": "parse-fail CSS",
        "heading": "Parse-fail CSS — invalid syntax drops the sheet",
        "expected": "css.served-but-not-applied",
        "css_links": ['<link rel="stylesheet" href="/parsefail.css">'],
    },
    "/no-stylesheets/": {
        "title": "no stylesheets",
        "heading": "No CSS at all — zero stylesheet links and zero style blocks",
        "expected": "css.no-stylesheets-declared (warn)",
        "css_links": [],
    },
}


class FixtureHandler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802
        path = self.path.split("?", 1)[0]
        if path in ROUTES:
            self._render_fixture(path)
        elif path == "/working.css":
            self._send(200, "text/css; charset=utf-8", WORKING_CSS)
        elif path == "/empty.css":
            self._send(200, "text/css; charset=utf-8", b"")
        elif path == "/wrongmime.css":
            # Lie about Content-Type. Browsers will refuse to apply.
            self._send(200, "text/html; charset=utf-8", WORKING_CSS)
        elif path == "/parsefail.css":
            self._send(200, "text/css; charset=utf-8", PARSEFAIL_CSS)
        elif path == "/":
            self._send(
                200,
                "text/html; charset=utf-8",
                self._index_page().encode(),
            )
        else:
            self._send(404, "text/html", b"<h1>404</h1>")

    def _render_fixture(self, path: str) -> None:
        spec = ROUTES[path]
        body = PAGE_TEMPLATE.format(
            title=spec["title"],
            css_links="\n".join(spec["css_links"]),
            heading=spec["heading"],
            expected_finding=spec["expected"],
        )
        self._send(200, "text/html; charset=utf-8", body.encode())

    def _index_page(self) -> str:
        rows = "\n".join(
            f'<li><a href="{p}">{spec["heading"]}</a> '
            f'— expected: <code>{spec["expected"]}</code></li>'
            for p, spec in ROUTES.items()
        )
        return f"""<!doctype html><html><head><title>cssHealth fixtures</title></head><body>
<h1>cssHealth fixture index</h1>
<ul>{rows}</ul>
</body></html>"""

    def _send(self, status: int, ctype: str, body: bytes) -> None:
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def log_message(self, fmt: str, *args: object) -> None:
        ts = time.strftime("%H:%M:%S")
        sys.stdout.write(f"[fixture {ts}] {self.address_string()} {fmt%args}\n")
        sys.stdout.flush()


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8766)
    args = ap.parse_args()
    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer(("", args.port), FixtureHandler) as httpd:
        print(f"[fixture] http://127.0.0.1:{args.port}/")
        httpd.serve_forever()


if __name__ == "__main__":
    main()
