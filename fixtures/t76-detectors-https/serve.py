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
_DEFAULT_PERMISSIONS_POLICY = ', '.join(
    f'{f}=()' for f in [
        'camera', 'microphone', 'geolocation', 'payment', 'usb', 'serial', 'midi',
        'hid', 'bluetooth', 'accelerometer', 'gyroscope', 'magnetometer',
        'display-capture', 'screen-wake-lock',
    ]
)

_DEFAULT_CSP = (
    "default-src 'self'; object-src 'none'; base-uri 'self'; "
    "form-action 'self'; frame-ancestors 'none'; "
    "require-trusted-types-for 'script'; script-src 'self'"
)

DEFAULT_HEADERS = {
    'Strict-Transport-Security': 'max-age=31536000; includeSubDomains',
    'X-Frame-Options': 'SAMEORIGIN',
    'Referrer-Policy': 'strict-origin-when-cross-origin',
    'Permissions-Policy': _DEFAULT_PERMISSIONS_POLICY,
    'Content-Security-Policy': _DEFAULT_CSP,
    'Cross-Origin-Opener-Policy': 'same-origin',
    'Cross-Origin-Embedder-Policy': 'require-corp',
    # cycle 31: Reporting API endpoint stub so unrelated routes
    # don't all leak reporting.no-endpoints. The URL is a
    # plausible-looking placeholder; the detector doesn't try
    # to validate the endpoint reachability.
    'Reporting-Endpoints': 'csp-default="https://reports.example.com/csp"',
    # cycle 45: Origin-Agent-Cluster — process-level isolation
    # primitive. '?1' is the supersociety baseline.
    'Origin-Agent-Cluster': '?1',
    'Content-Type': 'text/html; charset=utf-8',
    # Cache-Control: 'no-store' alone (without 'no-cache' or
    # 'max-age=0') is the canonical "do not cache" directive
    # since RFC 9111 superseded RFC 7234. The previous combo
    # value (no-store, no-cache, must-revalidate, max-age=0)
    # was historical IE6-era boilerplate; it's also a
    # cache-control.contradictory finding under the cycle-28
    # detector (no-store + max-age cancel).
    'Cache-Control': 'no-store',
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
# These routes must override Content-Security-Policy to None too —
# the default CSP carries frame-ancestors 'none', which supersedes
# X-Frame-Options. Stripping CSP isolates the XFO detector
# behaviour we want to verify.
@route('/no-xfo/')
def no_xfo():
    return (
        page('<h1>No X-Frame-Options + no CSP frame-ancestors.</h1>'),
        {'X-Frame-Options': None, 'Content-Security-Policy': None},
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
        {'X-Frame-Options': 'GARBAGE', 'Content-Security-Policy': None},
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


# ----- Content-Security-Policy -----
# Reuse _DEFAULT_CSP (already wired into DEFAULT_HEADERS) so the
# focused-finding routes mutate one directive at a time without
# drifting from the shared baseline.
_CSP_HARDENED = _DEFAULT_CSP


@route('/no-csp/')
def no_csp():
    # No CSP at all → warn csp.missing.
    return (
        page('<h1>No Content-Security-Policy header.</h1>'),
        {'Content-Security-Policy': None},
    )


@route('/csp-unsafe-inline/')
def csp_unsafe_inline():
    return (
        page('<h1>CSP allows script-src unsafe-inline.</h1>'),
        {'Content-Security-Policy':
            _CSP_HARDENED.replace("script-src 'self'", "script-src 'self' 'unsafe-inline'")},
    )


@route('/csp-unsafe-eval/')
def csp_unsafe_eval():
    return (
        page('<h1>CSP allows script-src unsafe-eval.</h1>'),
        {'Content-Security-Policy':
            _CSP_HARDENED.replace("script-src 'self'", "script-src 'self' 'unsafe-eval'")},
    )


@route('/csp-wildcard-script/')
def csp_wildcard_script():
    return (
        page('<h1>CSP script-src wildcard.</h1>'),
        {'Content-Security-Policy':
            _CSP_HARDENED.replace("script-src 'self'", 'script-src *')},
    )


@route('/csp-no-trusted-types/')
def csp_no_trusted_types():
    # Otherwise hardened, no require-trusted-types-for.
    return (
        page('<h1>CSP missing trusted-types directive.</h1>'),
        {'Content-Security-Policy':
            _CSP_HARDENED.replace("require-trusted-types-for 'script'; ", '')},
    )


@route('/csp-clean/')
def csp_clean():
    return (
        page('<h1>CSP hardened — clean baseline.</h1>'),
        {'Content-Security-Policy': _CSP_HARDENED},
    )


# ----- Origin-Agent-Cluster -----
@route('/no-origin-agent-cluster/')
def no_origin_agent_cluster():
    return (
        page('<h1>No Origin-Agent-Cluster header.</h1>'),
        {'Origin-Agent-Cluster': None},
    )


@route('/origin-agent-cluster-disabled/')
def origin_agent_cluster_disabled():
    return (
        page('<h1>Origin-Agent-Cluster explicitly disabled.</h1>'),
        {'Origin-Agent-Cluster': '?0'},
    )


@route('/origin-agent-cluster-invalid/')
def origin_agent_cluster_invalid():
    return (
        page('<h1>Origin-Agent-Cluster invalid value.</h1>'),
        {'Origin-Agent-Cluster': 'yes'},
    )


@route('/origin-agent-cluster-clean/')
def origin_agent_cluster_clean():
    return page('<h1>Origin-Agent-Cluster ?1 — clean.</h1>'), {}


# ----- Reporting API endpoints -----
@route('/reporting-no-endpoints/')
def reporting_no_endpoints():
    return (
        page('<h1>No Reporting-Endpoints, no Report-To.</h1>'),
        {'Reporting-Endpoints': None},
    )


@route('/reporting-report-to-only/')
def reporting_report_to_only():
    return (
        page('<h1>Legacy Report-To only.</h1>'),
        {
            'Reporting-Endpoints': None,
            'Report-To': '{"group":"csp","max_age":86400,"endpoints":[{"url":"https://reports.example.com/csp"}]}',
        },
    )


@route('/reporting-invalid/')
def reporting_invalid():
    return (
        page('<h1>Reporting-Endpoints garbage.</h1>'),
        {'Reporting-Endpoints': '   ,   ,   '},
    )


@route('/reporting-csp-orphan/')
def reporting_csp_orphan():
    # CSP references report-to but no Reporting-Endpoints.
    # Override the default CSP to add the orphan report-to,
    # AND null out Reporting-Endpoints to expose the orphan.
    csp_with_report = (
        "default-src 'self'; object-src 'none'; base-uri 'self'; "
        "form-action 'self'; frame-ancestors 'none'; "
        "require-trusted-types-for 'script'; script-src 'self'; "
        "report-to csp-default"
    )
    return (
        page('<h1>CSP report-to references orphan endpoint.</h1>'),
        {
            'Content-Security-Policy': csp_with_report,
            'Reporting-Endpoints': None,
        },
    )


@route('/reporting-clean/')
def reporting_clean():
    # Default Reporting-Endpoints from DEFAULT_HEADERS — passes.
    return page('<h1>Reporting-Endpoints clean.</h1>'), {}


# ----- Vary correctness -----
@route('/vary-no-cookie-with-set-cookie/')
def vary_no_cookie_with_set_cookie():
    # Set-Cookie + cacheable max-age + Vary missing → warn.
    return (
        page('<h1>Set-Cookie + max-age, no Vary: Cookie.</h1>'),
        {
            'Cache-Control': 'max-age=600',
            'Set-Cookie': 'sid=abc; Secure; HttpOnly; SameSite=Strict',
            'Vary': 'Accept-Encoding',
        },
    )


@route('/vary-star/')
def vary_star():
    return (
        page('<h1>Vary: *.</h1>'),
        {'Vary': '*'},
    )


@route('/vary-invalid/')
def vary_invalid():
    return (
        page('<h1>Vary garbage value.</h1>'),
        {'Vary': '@@@nope@@@'},
    )


@route('/vary-duplicate/')
def vary_duplicate():
    return (
        page('<h1>Vary with duplicate tokens.</h1>'),
        {'Vary': 'Cookie, cookie, Accept-Language'},
    )


@route('/vary-clean/')
def vary_clean():
    # Default no-store from DEFAULT_HEADERS — Set-Cookie isn't
    # cacheable so no Vary finding. (Add a no-Vary HTML page
    # that's clean — uses defaults.)
    return page('<h1>Vary clean — no Set-Cookie, no Vary needed.</h1>'), {}


# ----- Cache-Control hygiene -----
@route('/cache-control-missing/')
def cache_control_missing():
    return (
        page('<h1>No Cache-Control header.</h1>'),
        {'Cache-Control': None},
    )


@route('/cache-control-public-with-cookie/')
def cache_control_public_with_cookie():
    # Strict — Web Cache Deception risk.
    return (
        page('<h1>Cache-Control public + Set-Cookie.</h1>'),
        {
            'Cache-Control': 'public, max-age=3600',
            'Set-Cookie': 'sid=abc; Secure; HttpOnly; SameSite=Strict',
        },
    )


@route('/cache-control-no-private-with-cookie/')
def cache_control_no_private_with_cookie():
    # Warn — Set-Cookie + neither no-store nor private.
    return (
        page('<h1>Cache-Control max-age + Set-Cookie, no private.</h1>'),
        {
            'Cache-Control': 'max-age=600',
            'Set-Cookie': 'sid=abc; Secure; HttpOnly; SameSite=Strict',
        },
    )


@route('/cache-control-invalid/')
def cache_control_invalid():
    return (
        page('<h1>Cache-Control garbage value.</h1>'),
        {'Cache-Control': '   ,   ,   '},
    )


@route('/cache-control-unrealistic-maxage/')
def cache_control_unrealistic_maxage():
    return (
        page('<h1>Cache-Control max-age > 1 year.</h1>'),
        {'Cache-Control': 'public, max-age=99999999'},
    )


@route('/cache-control-contradictory/')
def cache_control_contradictory():
    return (
        page('<h1>Cache-Control no-store + max-age (contradictory).</h1>'),
        {'Cache-Control': 'no-store, max-age=60'},
    )


@route('/cache-control-clean/')
def cache_control_clean():
    # Default no-store from DEFAULT_HEADERS — passes cleanly.
    return page('<h1>Cache-Control clean — no-store.</h1>'), {}


# ----- Info-leak headers (opsec hygiene) -----
@route('/info-leak-server-version/')
def info_leak_server_version():
    return (
        page('<h1>Server header reveals version.</h1>'),
        {'Server': 'nginx/1.20.1'},
    )


@route('/info-leak-x-powered-by/')
def info_leak_x_powered_by():
    return (
        page('<h1>X-Powered-By header present.</h1>'),
        {'X-Powered-By': 'PHP/7.4.3'},
    )


@route('/info-leak-x-aspnet-version/')
def info_leak_x_aspnet_version():
    return (
        page('<h1>X-AspNet-Version header present.</h1>'),
        {'X-AspNet-Version': '4.0.30319'},
    )


@route('/info-leak-x-aspnetmvc-version/')
def info_leak_x_aspnetmvc_version():
    return (
        page('<h1>X-AspNetMvc-Version header present.</h1>'),
        {'X-AspNetMvc-Version': '5.2'},
    )


@route('/info-leak-x-runtime/')
def info_leak_x_runtime():
    return (
        page('<h1>X-Runtime header present.</h1>'),
        {'X-Runtime': '0.123456'},
    )


@route('/info-leak-x-debug-token/')
def info_leak_x_debug_token():
    return (
        page('<h1>X-Debug-Token (Symfony web-profiler).</h1>'),
        {'X-Debug-Token': 'ab12cd'},
    )


@route('/info-leak-via/')
def info_leak_via():
    return (
        page('<h1>Via header (intermediate proxy).</h1>'),
        {'Via': '1.1 internal-proxy.corp.example (varnish/6.0.8)'},
    )


@route('/info-leak-x-generator/')
def info_leak_x_generator():
    return (
        page('<h1>X-Generator (Drupal).</h1>'),
        {'X-Generator': 'Drupal 9 (https://www.drupal.org)'},
    )


@route('/info-leak-clean/')
def info_leak_clean():
    # No info-leak headers set. Server: 'web' bare product name
    # comes from the Handler class default and should NOT fire.
    return page('<h1>Info-leak clean — no version-disclosure headers.</h1>'), {}


# ----- Cross-Origin-Opener-Policy / Embedder-Policy -----
@route('/no-coop/')
def no_coop():
    return (
        page('<h1>No Cross-Origin-Opener-Policy.</h1>'),
        {'Cross-Origin-Opener-Policy': None},
    )


@route('/coop-unsafe-none/')
def coop_unsafe_none():
    return (
        page('<h1>COOP unsafe-none.</h1>'),
        {'Cross-Origin-Opener-Policy': 'unsafe-none'},
    )


@route('/coop-invalid/')
def coop_invalid():
    return (
        page('<h1>COOP unrecognised value.</h1>'),
        {'Cross-Origin-Opener-Policy': 'whatever-token'},
    )


@route('/no-coep/')
def no_coep():
    return (
        page('<h1>No Cross-Origin-Embedder-Policy.</h1>'),
        {'Cross-Origin-Embedder-Policy': None},
    )


@route('/coep-unsafe-none/')
def coep_unsafe_none():
    return (
        page('<h1>COEP unsafe-none.</h1>'),
        {'Cross-Origin-Embedder-Policy': 'unsafe-none'},
    )


@route('/coep-invalid/')
def coep_invalid():
    return (
        page('<h1>COEP unrecognised value.</h1>'),
        {'Cross-Origin-Embedder-Policy': 'wat'},
    )


# ----- Permissions-Policy -----
# Reuse the deny-all string already wired into DEFAULT_HEADERS so
# the "high-risk-omitted" warning doesn't pile on top of the
# focused finding under test.
PP_DENY_ALL = _DEFAULT_PERMISSIONS_POLICY


@route('/no-permissions-policy/')
def no_permissions_policy():
    return (
        page('<h1>No Permissions-Policy header.</h1>'),
        {'Permissions-Policy': None},
    )


@route('/permissions-policy-camera-allowall/')
def permissions_policy_camera_allowall():
    # camera=* allow-all → strict.
    return (
        page('<h1>Permissions-Policy explicitly allow-alls camera.</h1>'),
        {'Permissions-Policy': PP_DENY_ALL.replace('camera=()', 'camera=*')},
    )


@route('/permissions-policy-invalid/')
def permissions_policy_invalid():
    # Garbage value, no `=` separator anywhere.
    return (
        page('<h1>Permissions-Policy garbage value.</h1>'),
        {'Permissions-Policy': 'totally not a policy'},
    )


@route('/permissions-policy-partial/')
def permissions_policy_partial():
    # Declares non-high-risk features; high-risk omitted (default *).
    return (
        page('<h1>Permissions-Policy declares only non-high-risk features.</h1>'),
        {'Permissions-Policy': 'autoplay=(), fullscreen=(self)'},
    )


@route('/permissions-policy-clean/')
def permissions_policy_clean():
    # Comprehensive deny → no findings.
    return (
        page('<h1>Permissions-Policy comprehensive deny — clean.</h1>'),
        {'Permissions-Policy': PP_DENY_ALL},
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
    # Override the default `Server: BaseHTTP/0.6 Python/3.13.X`
    # auto-emit to a clean bare-product name. The infoLeakHeaders
    # detector specifically tests for VERSION tokens — the bare
    # name should pass cleanly so unrelated routes don't all
    # fire info-leak.server-version. Routes that want to test
    # that finding override the Server header explicitly via
    # the per-route header dict.
    server_version = 'web'
    sys_version = ''

    def date_time_string(self, timestamp=None):
        # Stable Date for reproducible audit output. Doesn't
        # affect the info-leak detector (it ignores Date) but
        # avoids cross-run noise in cookie-security examples.
        return super().date_time_string(timestamp)

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
