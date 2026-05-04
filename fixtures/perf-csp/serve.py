#!/usr/bin/env python3
"""serve.py — fixture server for performance + CSP detectors (T72).

Each route is a known-bad input that exercises ONE detector axis.
The detection-first contract: visiting these pages MUST produce
specific findings. If a detector goes silent on its own bait, the
detector is broken.

Routes:

  /clean/         — minimal HTML, no findings expected (control).
                    Verifies the fixture pipeline itself isn't
                    silently swallowing real findings.

  /long-task/     — synchronous script that blocks the main thread
                    for ~250ms via tight while-loop. Web-vitals
                    INP / LCP detectors should flag it.

  /oversize-js/   — inline <script> body padded to ~250KB. Pushes
                    HTML response well past the bundle-size budget;
                    cssHealth+perfBudget detectors should warn.

  /csp-violation/ — meta CSP with `script-src 'self'` and an inline
                    <script> the policy forbids. Browser blocks
                    execution AND emits a securitypolicyviolation
                    event; cspViolations detector should fire.

  /broken-href/   — <a href> pointing at a 404 plus an <img>
                    pointing at /missing.jpg. Failed-requests +
                    runtimeImages detectors should fire.

Run with::

    python3 fixtures/perf-csp/serve.py --port 8312

Then a journey targeting :8312 verifies every route produces the
expected finding. Useful as both a regression test for the
detectors AND as living documentation of what each axis catches.
"""

import argparse
import http.server
import socketserver
import sys

# Padding string for the oversize-js route. ~250 KB of harmless
# JavaScript that the browser parses but doesn't execute.
_BLOB_LINE = "// padding " + ("x" * 100) + "\n"
OVERSIZE_BLOB = "var __pad__ = 0;\n" + (_BLOB_LINE * 2400)

# Synchronous busy-loop for the long-task route. ~250ms on a
# typical CI box (under 50ms is the WCAG threshold for a long
# task; 250ms guarantees the web-vitals detector trips).
LONG_TASK_SCRIPT = (
    "var __t0__ = performance.now();"
    "while (performance.now() - __t0__ < 250) {"
    "  Math.sqrt(Math.random() * 1e6);"
    "}"
)

PAGE_TEMPLATE = """<!doctype html>
<html lang="en"><head>
<meta charset="utf-8">
<title>{title}</title>
{extra_head}
<style>body{{margin:0;padding:1rem;font-family:system-ui,sans-serif;background:#fff;color:#111}}</style>
</head><body>
<h1>{heading}</h1>
<p>Expected finding: <code>{expected}</code></p>
{body}
</body></html>
"""

ROUTES = {
    "/clean/": dict(
        title="clean",
        heading="Clean — no findings expected",
        expected="(silent — control)",
        extra_head="",
        body="<p>Just text. Nothing dynamic, no scripts, no images.</p>",
    ),
    "/long-task/": dict(
        title="long-task",
        heading="Long task — 250ms synchronous",
        expected="webVitals INP / long-task",
        extra_head="",
        body=(
            "<p>This page runs a 250ms busy-loop on the main thread. "
            "INP / TBT measurements should land in the poor band.</p>"
            f"<script>{LONG_TASK_SCRIPT}</script>"
        ),
    ),
    "/oversize-js/": dict(
        title="oversize-js",
        heading="Oversize JS — 250 KB inline blob",
        expected="perfBudget bundle-size",
        extra_head="",
        body=(
            "<p>Inline script padded to ~250 KB. Bundle-size budget "
            "should fail.</p>"
            f"<script>{OVERSIZE_BLOB}</script>"
        ),
    ),
    "/csp-violation/": dict(
        title="csp-violation",
        heading="CSP violation — inline <script> forbidden",
        expected="cspViolations + console-error",
        # T72: HTTP-header CSP (set in Handler.do_GET below) — meta
        # CSP doesn't reliably fire `securitypolicyviolation` events
        # in Chromium for blocked inline scripts. Header CSP does.
        extra_head="",
        body=(
            "<p>Inline script forbidden by header CSP.</p>"
            "<script>console.warn('this should never run');</script>"
        ),
    ),
    "/broken-href/": dict(
        title="broken-href",
        heading="Broken href + missing image",
        expected="failed-requests + runtimeImages",
        extra_head="",
        body=(
            '<p><a href="/404.html">link to nowhere</a></p>'
            '<img src="/missing.jpg" alt="missing" width="64" height="64">'
        ),
    ),
}


def render(path: str) -> bytes:
    spec = ROUTES.get(path)
    if spec is None:
        return b""
    return PAGE_TEMPLATE.format(**spec).encode("utf-8")


class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802
        path = self.path.split("?", 1)[0]
        if path == "/":
            # Index lists every route so a human can click through.
            rows = "".join(
                f'<li><a href="{p}">{s["heading"]}</a> '
                f'<small>expected: {s["expected"]}</small></li>'
                for p, s in ROUTES.items()
            )
            body = (
                "<!doctype html><meta charset=utf-8>"
                "<title>perf-csp fixture</title>"
                "<h1>perf + CSP fixture (T72)</h1>"
                f"<ul>{rows}</ul>"
            ).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if path in ROUTES:
            body = render(path)
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            # T72: route-specific CSP header so the inline-script
            # block fires `securitypolicyviolation` reliably
            # (header CSP > meta CSP for event firing in Chromium).
            if path == "/csp-violation/":
                self.send_header(
                    "Content-Security-Policy",
                    "default-src 'self'; script-src 'self'; "
                    "style-src 'self' 'unsafe-inline'",
                )
            self.end_headers()
            self.wfile.write(body)
            return
        # 404 routes (used by /broken-href/ and CSP report URI):
        # don't crash, just return a small body so the test
        # observation is "this returned 404" rather than a
        # connection error.
        self.send_response(404)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.end_headers()
        self.wfile.write(b"not found\n")

    def log_message(self, fmt: str, *args: object) -> None:
        # Quiet by default — the journey is the consumer.
        return


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8312)
    args = ap.parse_args()

    class ThreadedTCPServer(socketserver.ThreadingMixIn, socketserver.TCPServer):
        daemon_threads = True
        allow_reuse_address = True

    with ThreadedTCPServer(("", args.port), Handler) as httpd:
        sys.stdout.write(
            f"[fixtures/perf-csp] http://127.0.0.1:{args.port}/\n"
        )
        sys.stdout.flush()
        httpd.serve_forever()


if __name__ == "__main__":
    main()
