#!/usr/bin/env python3
"""serve.py — fixture server for the uiOverflow detector (T28).

Routes:

  /clean/                — no overflow, all tap targets >=44px (control)
  /page-h-scroll/        — fixed-width 2000px element forces page scroll
  /element-bleed/        — element pushed past viewport with no overflow:auto
  /text-clipped/         — long unbroken token clipped without scroll affordance
  /small-tap-targets/    — buttons 24×24
  /combined/             — combination of the above

Each page declares its expected finding kind in the body so the
operator can sanity-check by visiting in a browser.
"""
import argparse
import http.server
import socketserver
import sys
import time

PAGE = """<!doctype html>
<html lang=en><head>
<meta charset=utf-8>
<meta name=viewport content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
  body {{ margin: 0; padding: 1rem; font-family: system-ui, sans-serif; background: #f6f7fb; }}
  .page {{ max-width: 100%; }}
  h1 {{ font-size: 1.25rem; margin: 0 0 0.5rem; }}
  p  {{ margin: 0.25rem 0 0.75rem; color: #475569; }}
  code {{ background: #1e293b; color: #e2e8f0; padding: 0.1rem 0.4rem; border-radius: 4px; }}
  {extra_css}
</style>
</head><body>
<div class=page>
  <h1>{heading}</h1>
  <p>Expected finding: <code>{expected}</code></p>
  {body}
</div>
</body></html>
"""

ROUTES = {
    "/clean/": dict(
        title="clean",
        heading="Clean — no overflow, all tap targets ≥ 44px",
        expected="(no findings)",
        extra_css="""
            button.btn { display: inline-flex; align-items: center; min-width: 44px; min-height: 44px; padding: 0.5rem 1rem; }
            a.link { display: inline-block; min-width: 44px; min-height: 44px; padding: 0.5rem 1rem; }
        """,
        body="""
            <button class=btn>OK</button>
            <a class=link href=#>Home</a>
            <p>The quick brown fox jumps over the lazy dog. No overflow here.</p>
        """,
    ),
    "/page-h-scroll/": dict(
        title="page horizontal scroll",
        heading="Page horizontal scroll — 2000px fixed element",
        expected="overflow.page-horizontal-scroll",
        extra_css="""
            .wide { width: 2000px; height: 80px; background: linear-gradient(90deg, #f87171, #fbbf24); }
        """,
        body="""
            <div class=wide></div>
            <p>The .wide element is 2000px and forces a horizontal scrollbar.</p>
        """,
    ),
    "/element-bleed/": dict(
        title="element bleeds viewport",
        heading="Element bleeds viewport — absolutely positioned past right edge",
        expected="overflow.element-bleeds-viewport",
        extra_css="""
            .bleeder { position: absolute; left: 50%; top: 80px; width: 80vw; height: 60px; background: #34d399; }
        """,
        body="""
            <div class=bleeder>I extend past the right edge</div>
            <p style=margin-top:160px>An element 80vw wide starting at left:50% extends past the viewport.</p>
        """,
    ),
    "/text-clipped/": dict(
        title="text clipped",
        heading="Text clipped — long unbroken token in a fixed-width pre",
        expected="overflow.text-clipped",
        extra_css="""
            pre.fixed { width: 200px; font-family: monospace; background: #fff; padding: 0.5rem; border: 1px solid #cbd5e1; overflow: hidden; }
        """,
        body="""
            <pre class=fixed>aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa</pre>
            <p>The pre clips overflow:hidden — content extends past the visible area with no scroll affordance.</p>
        """,
    ),
    "/small-tap-targets/": dict(
        title="small tap targets",
        heading="Small tap targets — three buttons at 24×24",
        expected="overflow.tap-target-too-small",
        extra_css="""
            button.tiny { width: 24px; height: 24px; padding: 0; font-size: 10px; }
        """,
        body="""
            <button class=tiny>X</button>
            <button class=tiny>Y</button>
            <button class=tiny>Z</button>
            <p>Three buttons sized 24×24 (well below the 44×44 minimum).</p>
        """,
    ),
    "/combined/": dict(
        title="combined breakage",
        heading="Combined — page scroll + element bleed + small targets",
        expected="multiple findings",
        extra_css="""
            .wide { width: 1500px; height: 60px; background: #f87171; }
            button.tiny { width: 30px; height: 30px; padding: 0; }
        """,
        body="""
            <div class=wide></div>
            <button class=tiny>!</button>
            <p>Multiple offenders.</p>
        """,
    ),
}


class FixtureHandler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802
        path = self.path.split("?", 1)[0]
        if path == "/":
            rows = "\n".join(
                f'<li><a href="{p}">{spec["heading"]}</a> — '
                f'expected: <code>{spec["expected"]}</code></li>'
                for p, spec in ROUTES.items()
            )
            body = f"<!doctype html><html><body><h1>uiOverflow fixtures</h1><ul>{rows}</ul></body></html>"
            self._send(200, "text/html; charset=utf-8", body.encode())
            return
        if path in ROUTES:
            spec = ROUTES[path]
            body = PAGE.format(
                title=spec["title"],
                heading=spec["heading"],
                expected=spec["expected"],
                extra_css=spec["extra_css"],
                body=spec["body"],
            )
            self._send(200, "text/html; charset=utf-8", body.encode())
            return
        self._send(404, "text/html", b"<h1>404</h1>")

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
        sys.stdout.write(f"[ui-fix {ts}] {self.address_string()} {fmt%args}\n")
        sys.stdout.flush()


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8767)
    args = ap.parse_args()
    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer(("", args.port), FixtureHandler) as httpd:
        print(f"[ui-fix] http://127.0.0.1:{args.port}/")
        httpd.serve_forever()


if __name__ == "__main__":
    main()
