#!/usr/bin/env python3
"""serve.py — HTTPS fixture server for T76 response-header
detectors (hsts, xFrameOptions) + the mixedContent detector
that requires the page itself to be loaded over https.

Sister to fixtures/t76-detectors/serve.py (which is http-only).
The http fixture's localhost short-circuit means it can't
exercise hsts / xFrameOptions / mixedContent live; that gap
is closed here.

USAGE
-----
    CRAWLER_IGNORE_HTTPS_ERRORS=1 \\
        scripts/check-t76-detectors.sh --variant https

The CRAWLER_IGNORE_HTTPS_ERRORS=1 env var is REQUIRED — the
fixture's cert is self-signed and would otherwise tank the
audit.

CERT
----
A self-signed RSA-2048 cert pinned to CN=localhost lives in
this directory (cert.pem + key.pem). Generated with:

    openssl req -x509 -newkey rsa:2048 -keyout key.pem -out cert.pem \\
        -sha256 -days 36500 -nodes -subj "/CN=localhost"

100-year validity so it doesn't expire mid-CI. The cert is for
fixture testing only; never reuse the key for anything that
matters.

ROUTES
------
The detectors short-circuit the localhost check on `localhost`
and `127.0.0.1` and `::1`. To EXERCISE the detectors, the
fixture publishes routes that look like a public-domain
deployment to the URL-parsing logic. Two strategies:

1. Serve from `127.0.0.1` but the page-side scripts also
   need to see it as non-localhost. We use `*.localhost.invalid`
   in the URLs but bind to `127.0.0.1`. Playwright treats the
   hostname literally for the `URL().hostname` check.

   For HTTPS to work with the cert, Playwright is told via
   network override — see `--network-rule` in scripts/...

2. Easier alternative: have the detectors take a config flag
   to disable the localhost check for fixture mode. Not done
   here — keeps the production behaviour intact.

For now the simplest workable thing: bind to `127.0.0.1`,
issue the cert with `subjectAltName=DNS:localhost,IP:127.0.0.1`,
and pre-launch override the detector's localhost-skip via env
var. Done in a follow-up; v1 just establishes the cert + server
infrastructure.

V1 ROUTES
---------
  /control/             control — should produce no findings
  /no-hsts/             explicit response with NO Strict-Transport-Security
  /short-hsts/          max-age=3600 (< 6 months)
  /no-subdomains/       max-age=31536000 but no includeSubDomains
  /no-xfo/              no X-Frame-Options + no CSP frame-ancestors
  /allowall-frame/      CSP frame-ancestors '*'
  /invalid-xfo/         X-Frame-Options: GARBAGE
  /mixed-active/        page has http://other/script.js (mixed-content.active)
  /mixed-passive/       page has http://other/img.png (mixed-content.passive)
"""
import argparse
import http.server
import os
import socketserver
import ssl
import sys

CERT_DIR = os.path.dirname(os.path.abspath(__file__))
CERT_PATH = os.path.join(CERT_DIR, 'cert.pem')
KEY_PATH = os.path.join(CERT_DIR, 'key.pem')

# A 1px favicon so favicon.missing-link doesn't trip site-wide.
FAVICON_TAG = '<link rel="icon" href="data:image/svg+xml,%3Csvg/%3E">'


def page(body, *, extra_head=''):
    """Build a minimal valid HTML5 page. The HEAD includes the
    project-required favicon tag + a viewport meta + a short
    title — enough that NO per-page detectors fire false positives.
    Description is intentionally omitted so the metaDescription
    detector stays silent (the http fixture covers that)."""
    return f'''<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="description" content="An https fixture page for the response-header detector tests, well within bounds.">
{FAVICON_TAG}
<title>HTTPS Fixture {body[:20]}</title>
{extra_head}
</head>
<body>
<a href="#main" style="display:inline-block;padding:12px 16px;min-width:44px;min-height:44px">Skip to main content</a>
<main id="main">{body}</main>
</body>
</html>'''


# ============================================================
# Routes — each closure returns (body_html, extra_response_headers).
# The Handler attaches the extra headers AND its own per-route
# defaults. The DEFAULT response includes the protective headers
# (HSTS / XFO / etc.) so the /control/ route is clean.
# ============================================================
ROUTES = {}


def route(path):
    def deco(fn):
        ROUTES[path] = fn
        return fn
    return deco


# Default protective headers applied to every route UNLESS the
# route returns its own override (see Handler.do_GET below).
DEFAULT_HEADERS = {
    'Strict-Transport-Security': 'max-age=31536000; includeSubDomains',
    'X-Frame-Options': 'SAMEORIGIN',
    'Referrer-Policy': 'strict-origin-when-cross-origin',
    'Content-Type': 'text/html; charset=utf-8',
    'Cache-Control': 'no-store, no-cache, must-revalidate, max-age=0',
}


@route('/control/')
def control():
    return page('<h1>Control — clean baseline. All protective headers set.</h1>'), {}


# ----- HSTS -----
@route('/no-hsts/')
def no_hsts():
    # Override: explicitly omit Strict-Transport-Security.
    return (
        page('<h1>No HSTS header. Should fire hsts.missing.</h1>'),
        {'Strict-Transport-Security': None},  # None = remove from response
    )


@route('/short-hsts/')
def short_hsts():
    return (
        page('<h1>HSTS max-age too short.</h1>'),
        {'Strict-Transport-Security': 'max-age=3600; includeSubDomains'},
    )


@route('/no-subdomains/')
def no_subdomains():
    return (
        page('<h1>HSTS missing includeSubDomains.</h1>'),
        {'Strict-Transport-Security': 'max-age=31536000'},
    )


# ----- X-Frame-Options -----
@route('/no-xfo/')
def no_xfo():
    return (
        page('<h1>No X-Frame-Options + no CSP frame-ancestors.</h1>'),
        {'X-Frame-Options': None},
    )


@route('/allowall-frame/')
def allowall_frame():
    # CSP frame-ancestors * supersedes XFO; warn fires.
    return (
        page('<h1>frame-ancestors *.</h1>'),
        {'X-Frame-Options': None,
         'Content-Security-Policy': 'frame-ancestors *'},
    )


@route('/invalid-xfo/')
def invalid_xfo():
    return (
        page('<h1>Invalid X-Frame-Options value.</h1>'),
        {'X-Frame-Options': 'GARBAGE'},
    )


# ----- Referrer-Policy -----
@route('/no-referrer-policy/')
def no_referrer_policy():
    return (
        page('<h1>No Referrer-Policy header.</h1>'),
        {'Referrer-Policy': None},
    )


@route('/permissive-referrer/')
def permissive_referrer():
    return (
        page('<h1>Permissive referrer policy: unsafe-url.</h1>'),
        {'Referrer-Policy': 'unsafe-url'},
    )


@route('/invalid-referrer/')
def invalid_referrer():
    return (
        page('<h1>Invalid Referrer-Policy value.</h1>'),
        {'Referrer-Policy': 'GIBBERISH'},
    )


# ----- Cookie security (Set-Cookie attribute audit) -----
@route('/cookie-no-secure/')
def cookie_no_secure():
    # https + Secure missing → strict cookie.no-secure.
    # SameSite present so the warn doesn't pile on.
    return (
        page('<h1>Cookie set without Secure on https.</h1>'),
        {'Set-Cookie': 'foo=bar; SameSite=Lax'},
    )


@route('/cookie-no-samesite/')
def cookie_no_samesite():
    # No SameSite → warn cookie.no-samesite. Secure present.
    return (
        page('<h1>Cookie set without SameSite.</h1>'),
        {'Set-Cookie': 'foo=bar; Secure'},
    )


@route('/cookie-samesite-none-no-secure/')
def cookie_samesite_none_no_secure():
    # SameSite=None without Secure → strict (browsers reject).
    return (
        page('<h1>SameSite=None without Secure.</h1>'),
        {'Set-Cookie': 'cross=ok; SameSite=None'},
    )


@route('/cookie-session-no-httponly/')
def cookie_session_no_httponly():
    # Session-named cookie without HttpOnly → warn.
    return (
        page('<h1>Session cookie without HttpOnly.</h1>'),
        {'Set-Cookie': 'sessid=abc; Secure; SameSite=Lax'},
    )


@route('/cookie-clean/')
def cookie_clean():
    # Control: every attribute set correctly.
    return (
        page('<h1>Cookie clean — Secure + HttpOnly + SameSite=Strict.</h1>'),
        {'Set-Cookie': 'sid=abc; Secure; HttpOnly; SameSite=Strict; Path=/'},
    )


# ----- mixedContent -----
@route('/mixed-active/')
def mixed_active():
    body = '<h1>Mixed active content</h1><p>The script tag below loads over http on this https page.</p><script src="http://other.example/x.js"></script>'
    return page(body), {}


@route('/mixed-passive/')
def mixed_passive():
    body = '<h1>Mixed passive content</h1><p><img src="http://other.example/x.png" alt="x"></p>'
    return page(body), {}


# ============================================================
# Server
# ============================================================
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        entry = ROUTES.get(self.path)
        if entry is None:
            if self.path in ('/', ''):
                self._listing()
                return
            self.send_response(404)
            self.send_header('Content-Type', 'text/plain')
            self.end_headers()
            self.wfile.write(b'route not found')
            return
        result = entry()
        if isinstance(result, tuple):
            body, overrides = result
        else:
            body, overrides = result, {}
        # Compose final headers: defaults + overrides. Override
        # value of None means "remove from response".
        final_headers = dict(DEFAULT_HEADERS)
        for k, v in overrides.items():
            if v is None:
                final_headers.pop(k, None)
            else:
                final_headers[k] = v
        self.send_response(200)
        for k, v in final_headers.items():
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body.encode('utf-8'))

    def _listing(self):
        body = '<!doctype html><html><head><title>HTTPS Fixtures</title></head><body><h1>HTTPS fixtures</h1><ul>'
        for p in sorted(ROUTES.keys()):
            body += f'<li><a href="{p}">{p}</a></li>'
        body += '</ul></body></html>'
        self.send_response(200)
        self.send_header('Content-Type', 'text/html')
        self.end_headers()
        self.wfile.write(body.encode())

    def log_message(self, fmt, *args):
        return


class ReusableTLSServer(socketserver.TCPServer):
    """SO_REUSEADDR + TLS wrap so back-to-back fixture runs don't
    TIME_WAIT-deadlock the port. Mirrors the http fixture's
    ReusableTCPServer."""
    allow_reuse_address = True


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--port', type=int, default=8773)
    args = p.parse_args()
    if not os.path.exists(CERT_PATH) or not os.path.exists(KEY_PATH):
        print(f'[t76-https] FATAL: missing {CERT_PATH} or {KEY_PATH}', file=sys.stderr)
        sys.exit(1)
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(certfile=CERT_PATH, keyfile=KEY_PATH)
    print(f'[t76-https] serving {len(ROUTES)} routes on https://127.0.0.1:{args.port}/', flush=True)
    with ReusableTLSServer(('127.0.0.1', args.port), Handler) as httpd:
        httpd.socket = ctx.wrap_socket(httpd.socket, server_side=True)
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print('[t76-https] shutting down', flush=True)
            sys.exit(0)


if __name__ == '__main__':
    main()
