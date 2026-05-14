#!/usr/bin/env python3
"""serve.py — fixture server for T76 detectors.

Each route engineered to trigger one (or more) specific finding kinds
across the T76 detector axes. The crawler runs against this server
to prove every detection arm fires when it should AND stays silent
when it should — a regression guard against the silent-pass class
of bugs (e.g. the 2026-05-14 NaN evalFn bug where the formLabels
detector silently failed across every page without anyone noticing
until a real-site dogfood run surfaced the symptom).

Routes (path → expected finding kinds):

  /control/                 (none — clean baseline page)

  Page-meta family:
  /no-viewport/             viewport.missing
  /viewport-no-device/      viewport.no-device-width
  /viewport-zoom-disabled/  viewport.zoom-disabled
  /no-title/                title.missing
  /title-empty/             title.empty
  /title-generic/           title.generic
  /title-too-short/         title.too-short
  /title-too-long/          title.too-long
  /no-lang/                 lang.missing
  /lang-empty/              lang.empty
  /lang-invalid/            lang.invalid
  /lang-unknown/            lang.unknown-primary
  /no-meta-desc/            meta-description.missing
  /meta-desc-empty/         meta-description.empty
  /meta-desc-too-short/     meta-description.too-short
  /meta-desc-too-long/      meta-description.too-long

  Forms / interactivity:
  /no-skip-link/            skip.missing
  /skip-broken-target/      skip.broken-target
  /skip-not-first/          skip.not-first-focusable
  /form-no-label/           form.no-label (strict)
  /form-placeholder-only/   form.placeholder-only-label
  /form-required-no-mark/   form.required-no-indicator
  /tap-tiny/                tap.too-small
  /tap-small/               tap.below-recommended
  /autocomplete-creds/      autocomplete.missing-credentials (strict)
  /autocomplete-pii/        autocomplete.missing-pii
  /autocomplete-bogus/      autocomplete.invalid-token

  Security:
  /tabnab/                  link.tabnab-vulnerable (strict)
  /opener-explicit/         link.opener-explicit (strict)

USAGE
-----
    python3 serve.py --port 8771

The matching crawler journey at journeys/t76-detector-fixtures.json
visits every route and the post-run check (CI script) asserts that
the report.events stream contains the expected finding kind for
each page.
"""
import argparse
import http.server
import socketserver
import sys

# A "clean" head — every page that doesn't deliberately break a
# specific axis uses this so OTHER detectors stay silent and we
# can isolate exactly one signal per fixture.
# A 1px inline SVG so the favicon detector sees a present <link>
# without any network fetch. Used by every fixture page EXCEPT
# the no-favicon route which deliberately omits it.
FAVICON_TAG = '<link rel="icon" href="data:image/svg+xml,&lt;svg xmlns=\'http://www.w3.org/2000/svg\' viewBox=\'0 0 16 16\'&gt;&lt;circle cx=\'8\' cy=\'8\' r=\'6\' fill=\'%23333\'/&gt;&lt;/svg&gt;">'

CLEAN_HEAD = f"""<meta charset=utf-8>
<meta name=viewport content="width=device-width, initial-scale=1">
<meta name=description content="A reasonable summary that fits in the search-result preview window.">
{FAVICON_TAG}
<title>T76 Fixture</title>"""

# Used in routes that test viewport/title/lang/etc. specifically:
# omit the field-under-test, keep the rest healthy.
_OMIT = object()  # sentinel: drop this element entirely.

def head(*, viewport=None, title=None, meta_desc=None):
    """Build a head block.

    For each kwarg:
      - omitted (default None) → use the clean baseline value
      - explicit string value → override with that value
      - sentinel _OMIT → drop the element entirely (test "missing")

    The sentinel pattern avoids the False/None ambiguity that bit
    the first version: `False is not None` evaluates True, so a
    `title=False` argument was silently producing `<title>False</title>`.
    """
    parts = ['<meta charset=utf-8>']
    if viewport is _OMIT:
        pass
    elif viewport is None:
        parts.append('<meta name=viewport content="width=device-width, initial-scale=1">')
    else:
        parts.append(f'<meta name=viewport content="{viewport}">')
    if meta_desc is _OMIT:
        pass
    elif meta_desc is None:
        parts.append('<meta name=description content="A reasonable summary that fits the search-result preview window.">')
    else:
        parts.append(f'<meta name=description content="{meta_desc}">')
    if title is _OMIT:
        pass
    elif title is None:
        parts.append('<title>T76 Fixture</title>')
    else:
        parts.append(f'<title>{title}</title>')
    # Always include a favicon link so head()-built pages don't
    # spuriously trip favicon.missing-link. The no-favicon route
    # builds its head inline to deliberately omit this.
    parts.append(FAVICON_TAG)
    return '\n'.join(parts)


def page(body, *, lang='en', head_override=None):
    """Wrap body in a complete HTML document. Pass lang=None to
    omit the lang attribute entirely (test lang.missing)."""
    h = head_override if head_override is not None else CLEAN_HEAD
    if lang is None:
        html_open = '<html>'
    else:
        html_open = f'<html lang="{lang}">'
    # Inline-style the skip link so its bounding box meets the
    # 44×44 tap-target floor — without this the link renders as
    # default-text size (~80×16) and trips tap.too-small on every
    # page including the control. We don't use external CSS in
    # the fixture by design (each route should isolate one signal).
    skip_link = '<a class="skip" href="#main" style="display:inline-block;padding:12px 16px;min-width:44px;min-height:44px;box-sizing:border-box">Skip to main content</a>'
    return f'<!doctype html>\n{html_open}<head>\n{h}\n</head>\n<body>{skip_link}<main id="main">{body}</main></body></html>'


# ============================================================
# Route handlers
# ============================================================
ROUTES = {}

def route(path):
    def deco(fn):
        ROUTES[path] = fn
        return fn
    return deco


@route('/control/')
def control():
    return page('<h1>Control — clean baseline</h1><p>No findings expected on this page.</p>')


# ----- viewport family -----
@route('/no-viewport/')
def no_viewport():
    return page('<h1>No viewport meta</h1>',
                head_override=f'<meta charset=utf-8><title>T76 Fixture</title><meta name=description content="A reasonable summary that fits the search-result preview window.">{FAVICON_TAG}')


@route('/viewport-no-device/')
def viewport_no_device():
    return page('<h1>Viewport without width=device-width</h1>',
                head_override=head(viewport='initial-scale=1'))


@route('/viewport-zoom-disabled/')
def viewport_zoom_disabled():
    return page('<h1>Viewport disables zoom</h1>',
                head_override=head(viewport='width=device-width, initial-scale=1, user-scalable=no'))


# ----- title family -----
@route('/no-title/')
def no_title():
    return page('<h1>No title</h1>',
                head_override=head(title=_OMIT))


@route('/title-empty/')
def title_empty():
    return page('<h1>Empty title</h1>',
                head_override=head(title=''))


@route('/title-generic/')
def title_generic():
    return page('<h1>Generic title</h1>',
                head_override=head(title='Untitled'))


@route('/title-too-short/')
def title_too_short():
    return page('<h1>Tiny title</h1>',
                head_override=head(title='OK'))


@route('/title-too-long/')
def title_too_long():
    very_long = 'A long page title that exceeds the seventy-character search-engine truncation point by a comfortable margin'
    return page('<h1>Long title</h1>',
                head_override=head(title=very_long))


# ----- lang family -----
@route('/no-lang/')
def no_lang():
    return page('<h1>No html lang</h1>', lang=None)


@route('/lang-empty/')
def lang_empty():
    return page('<h1>Empty lang</h1>', lang='')


@route('/lang-invalid/')
def lang_invalid():
    return page('<h1>Invalid BCP-47</h1>', lang='en_US')


@route('/lang-unknown/')
def lang_unknown():
    # Structurally valid 2-letter, not in common ISO 639-1 set.
    return page('<h1>Unknown primary subtag</h1>', lang='xx')


# ----- meta-description family -----
@route('/no-meta-desc/')
def no_meta_desc():
    return page('<h1>No meta description</h1>',
                head_override=head(meta_desc=_OMIT))


@route('/meta-desc-empty/')
def meta_desc_empty():
    return page('<h1>Empty meta description</h1>',
                head_override=head(meta_desc=''))


@route('/meta-desc-too-short/')
def meta_desc_too_short():
    return page('<h1>Short meta description</h1>',
                head_override=head(meta_desc='Too brief.'))


@route('/meta-desc-too-long/')
def meta_desc_too_long():
    long_desc = 'A meta description that runs on and on past the point at which any search engine would truncate it for the result snippet preview, exceeding the conventional 160-character ceiling that Google enforces.'
    return page('<h1>Long meta description</h1>',
                head_override=head(meta_desc=long_desc))


# ----- skip-link family -----
@route('/no-skip-link/')
def no_skip_link():
    body = '<h1>No skip link</h1><nav><a href="/a">A</a><a href="/b">B</a></nav>'
    # Override page() to omit the skip-link wrapper
    return f'<!doctype html>\n<html lang="en"><head>\n{CLEAN_HEAD}\n</head>\n<body><nav>top nav</nav><main id="main">{body}</main></body></html>'


@route('/skip-broken-target/')
def skip_broken_target():
    # Skip link points at #content but the main has id=main
    body = '<h1>Skip link points at non-existent id</h1>'
    return f'<!doctype html>\n<html lang="en"><head>\n{CLEAN_HEAD}\n</head>\n<body><a class="skip" href="#content" style="display:inline-block;padding:12px 16px;min-width:44px;min-height:44px;box-sizing:border-box">Skip to main content</a><main id="main">{body}</main></body></html>'


@route('/skip-not-first/')
def skip_not_first():
    # Logo link comes BEFORE skip link in tab order
    body = '<h1>Skip link not first focusable</h1>'
    return f'<!doctype html>\n<html lang="en"><head>\n{CLEAN_HEAD}\n</head>\n<body><a href="/" style="display:inline-block;padding:12px 16px;min-width:44px;min-height:44px;box-sizing:border-box">Logo</a> <a class="skip" href="#main" style="display:inline-block;padding:12px 16px;min-width:44px;min-height:44px;box-sizing:border-box">Skip to main content</a><main id="main">{body}</main></body></html>'


# ----- form-labels family -----
@route('/form-no-label/')
def form_no_label():
    return page('<h1>Form input with no accessible name</h1><form><input type=text name=foo></form>')


@route('/form-placeholder-only/')
def form_placeholder_only():
    return page('<h1>Placeholder is the only label</h1><form><input type=text name=email placeholder="Your email"></form>')


@route('/form-required-no-mark/')
def form_required_no_mark():
    return page('<h1>Required field with no visible indicator</h1><form><label for=q>Question</label><input type=text id=q name=q required></form>')


# ----- tap-targets family -----
@route('/tap-tiny/')
def tap_tiny():
    # Inline style overrides so the button is exactly 12x12.
    return page('<h1>Tiny tap target</h1><p><button style="width:12px;height:12px;padding:0;border:1px solid;">X</button></p>')


@route('/tap-small/')
def tap_small():
    return page('<h1>Below-recommended tap target</h1><p><button style="width:30px;height:30px;padding:0;border:1px solid;">M</button></p>')


# ----- autocomplete family -----
@route('/autocomplete-creds/')
def autocomplete_creds():
    return page('<h1>Login form without autocomplete</h1><form><label for=user>Username</label><input id=user name=username type=text><label for=pw>Password</label><input id=pw name=password type=password></form>')


@route('/autocomplete-pii/')
def autocomplete_pii():
    return page('<h1>PII form without autocomplete</h1><form><label for=fn>Full name</label><input id=fn name=name type=text><label for=tel>Phone</label><input id=tel name=phone type=tel></form>')


@route('/autocomplete-bogus/')
def autocomplete_bogus():
    return page('<h1>Invalid autocomplete token</h1><form><label for=q>Comment</label><input id=q name=comment type=text autocomplete=BOGUS></form>')


# ----- favicon family -----
@route('/no-favicon/')
def no_favicon():
    # Head identical to CLEAN_HEAD but with FAVICON_TAG omitted —
    # this is the ONE route that should trigger favicon.missing-link.
    no_icon_head = '<meta charset=utf-8>\n<meta name=viewport content="width=device-width, initial-scale=1">\n<meta name=description content="A reasonable summary that fits the search-result preview window.">\n<title>T76 Fixture</title>'
    return page('<h1>No favicon link</h1>', head_override=no_icon_head)


# ----- security: outbound links -----
@route('/tabnab/')
def tabnab():
    return page('<h1>Tabnabbing-vulnerable link</h1><p><a href="https://other.example/" target="_blank">External</a></p>')


@route('/opener-explicit/')
def opener_explicit():
    return page('<h1>Explicit rel=opener</h1><p><a href="https://other.example/" target="_blank" rel="opener">External</a></p>')


# ============================================================
# Server
# ============================================================
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = ROUTES.get(self.path)
        if body is None:
            # / and any unmatched path → list of routes (so curl /
            # gives a usable index).
            if self.path == '/' or self.path == '':
                listing = '<!doctype html><html lang=en><head><meta charset=utf-8><title>T76 Fixtures</title><meta name=viewport content="width=device-width,initial-scale=1"><meta name=description content="Index of T76 detector fixture routes."></head><body><h1>T76 detector fixtures</h1><ul>'
                for path in sorted(ROUTES.keys()):
                    listing += f'<li><a href="{path}">{path}</a></li>'
                listing += '</ul></body></html>'
                self.send_response(200)
                self.send_header('Content-Type', 'text/html; charset=utf-8')
                self.end_headers()
                self.wfile.write(listing.encode('utf-8'))
                return
            self.send_response(404)
            self.send_header('Content-Type', 'text/plain; charset=utf-8')
            self.end_headers()
            self.wfile.write(b'route not found')
            return
        out = body() if callable(body) else body
        self.send_response(200)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        # Disable caching so the crawler always sees the latest source.
        self.send_header('Cache-Control', 'no-store, no-cache, must-revalidate, max-age=0')
        self.end_headers()
        self.wfile.write(out.encode('utf-8'))

    def log_message(self, fmt, *args):
        # Silence per-request stderr lines so the crawler output
        # stays readable.
        return


class ReusableTCPServer(socketserver.TCPServer):
    """SO_REUSEADDR so back-to-back fixture runs don't hit
    `OSError: [Errno 98] Address already in use` while the previous
    listener's socket is still in TIME_WAIT (60s on Linux). The CI
    runner restarts this server many times per session."""
    allow_reuse_address = True


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--port', type=int, default=8771)
    args = p.parse_args()
    print(f'[t76-fixtures] serving {len(ROUTES)} routes on http://127.0.0.1:{args.port}/', flush=True)
    with ReusableTCPServer(('127.0.0.1', args.port), Handler) as httpd:
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print('[t76-fixtures] shutting down', flush=True)
            sys.exit(0)


if __name__ == '__main__':
    main()
