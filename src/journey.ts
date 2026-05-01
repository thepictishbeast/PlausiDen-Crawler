/**
 * Journey — a scripted sequence of interactions the crawler runs against
 * a target URL. Each step declares what to click/wait/type/assert, and
 * the runner records events around it.
 *
 * Simple plain-data format so the same journey file can be authored by
 * hand, generated from a test recorder, or checked into a repo for
 * regression diffing.
 */
import type { Page } from 'playwright';

export type StepKind =
  | 'goto'
  | 'click'
  | 'fill'
  | 'type'
  | 'press'
  | 'wait'
  | 'waitForSelector'
  | 'screenshot'
  | 'assertText'
  | 'assertUrl'
  | 'assertQR'
  | 'scroll'
  | 'reload'
  | 'discover'
  | 'probe'
  | 'stress';

export interface Step {
  kind: StepKind;
  /** CSS selector for click/fill/type/waitForSelector/assertText. */
  selector?: string;
  /** Text value for fill/type/assertText. */
  text?: string;
  /** Key for press (e.g. 'Enter', 'Escape', 'Tab'). */
  key?: string;
  /** URL for goto. */
  url?: string;
  /** Label used in the report and as screenshot filename prefix. */
  label?: string;
  /** Per-step timeout override (ms). Defaults to 10_000. */
  timeout?: number;
  /** For wait: ms to sleep. */
  ms?: number;
  /** For scroll: vertical pixel delta (negative scrolls up). */
  dy?: number;
  /**
   * For discover: BFS configuration. The runner switches to autonomous
   * mode and crawls same-origin links from the current URL (or step.url
   * if provided), capturing aria + screenshot + interactables on every
   * page reached. See src/discover.ts for the full DiscoverConfig shape.
   */
  discover?: {
    maxPages?: number;
    maxDepth?: number;
    sameOrigin?: boolean;
    includePatterns?: string[];
    denyPatterns?: string[];
    interactButtons?: 'never' | 'safe-buttons' | 'all-buttons';
    settleMs?: number;
    navTimeoutMs?: number;
  };
  /**
   * For probe: adversarial URL-parameter mutation. Takes URLs (explicit
   * + harvested from a prior discover step's discover-pages.json) and
   * rewrites int/hex/UUID-shaped path segments into malformed variants
   * (null bytes, oversize, traversal, type-confusion). 5xx responses,
   * unexpected 200s on garbage input, body echoes of injected payloads,
   * and partial-entropy hash leaks become CapturedEvent findings. See
   * src/probe.ts for the full ProbeConfig shape.
   */
  probe?: {
    urls?: string[];
    mutators?: Array<'int' | 'hex' | 'uuid' | 'string'>;
    maxRequestsPerUrl?: number;
    maxRequestsTotal?: number;
    timeoutMs?: number;
    failStatuses?: number[];
    okStatuses?: number[];
    hashLeakPrefixes?: string[];
    includePatterns?: string[];
    denyPatterns?: string[];
    inheritDiscoverUrls?: boolean;
    headerSmuggling?: boolean;
    methodFuzz?: boolean;
    statelessGet?: boolean;
    statelessGetFields?: string[];
  };
  /**
   * For stress: aggressive UI-fuzz on the current page. Hammers every
   * visible interactable from many angles: random-order clicks, edge-case
   * form fills (empty / max-length / unicode / XSS / SQLi / control
   * chars), keyboard fuzz (Tab cycle, Escape spam, Enter, arrow keys),
   * viewport thrash (random resizes during interaction), rapid back/
   * forward, and (optionally) network chaos (random throttle / fail).
   *
   * Designed to surface: layout shift, focus traps that don't trap,
   * unguarded XSS sinks, double-submit handlers that race, modal close
   * leaks, useEffect cleanups that don't, and any selector that breaks
   * Playwright's CSS parser. The full event stream still flows through
   * the standard report so any console error / page error / failed fetch
   * surfaced under stress is diffed against the baseline.
   *
   * Defaults are aggressive on purpose. Tune `intensity` down for slow
   * pages or up for "really stress the UI" runs.
   */
  stress?: {
    /** "low" / "medium" / "high" / "extreme" — scales every count below. Default: "high". */
    intensity?: 'low' | 'medium' | 'high' | 'extreme';
    /** Total wall-clock budget in ms. Stress loops abort when this is hit. Default: 60_000. */
    durationMs?: number;
    /** How many random clicks to fire across the page. Scales with intensity. Default: 80. */
    clickCount?: number;
    /** How many form fills to perform. Each picks a random input + random edge-case payload. Default: 60. */
    fillCount?: number;
    /** How many keyboard-fuzz bursts (Tab/Escape/Enter/arrow). Default: 40. */
    keyboardCount?: number;
    /** How many viewport resize thrashes (random within sane bounds). Default: 12. */
    resizeCount?: number;
    /** Take a screenshot every N stress actions for visual diff. 0 = no screenshots. Default: 20. */
    screenshotEveryN?: number;
    /** Trigger network chaos (random throttle / abort / 500). Default: false. */
    networkChaos?: boolean;
    /**
     * Selectors to AVOID clicking — destructive UI like "Delete account"
     * buttons. Matched as Playwright selectors. Default: nav-away-from-test.
     */
    avoidSelectors?: string[];
    /**
     * Selectors of TABS / accordions to deliberately rotate through during
     * stress (so the crawler exercises the full tab strip, not just the
     * default tab). Each is clicked between fuzz bursts.
     */
    rotateTabs?: string[];
    /**
     * Custom edge-case payloads to mix into the form-fuzz pool. Defaults
     * cover the OWASP top hits (XSS / SQLi / NULL byte / RTL / surrogate
     * pair / max-length). Adding domain-specific payloads tightens the
     * crawl for project-specific input handlers.
     */
    extraPayloads?: string[];
    /** Skip click-fuzz (e.g. when you only want form-fuzz). Default: false. */
    skipClicks?: boolean;
    /** Skip form-fuzz. Default: false. */
    skipFills?: boolean;
    /** Skip keyboard-fuzz. Default: false. */
    skipKeyboard?: boolean;
    /** Skip viewport-fuzz. Default: false. */
    skipResize?: boolean;
    /** Take a final full-page screenshot at end. Default: true. */
    finalScreenshot?: boolean;
  };
}

export interface Journey {
  name: string;
  description?: string;
  /** Baseline URL — goto steps without a url prefix use this. */
  baseUrl: string;
  steps: Step[];
  /**
   * Path to a Playwright storageState JSON (cookies + localStorage) captured
   * from a prior interactive login. Loaded into the browser context before
   * any step runs, so authenticated journeys can crawl admin/voter views
   * without ever inserting fake credentials into the prod database.
   */
  storageState?: string;
  /**
   * Preflight `/health` check. The runner hits each URL once before any
   * step runs and aborts with exit code 3 if any returns non-2xx or fails
   * to connect. Catches "service is down" before we waste a full crawl
   * on opaque downstream failures (e.g. AppArmor denial in #279).
   */
  preflight?: { name: string; url: string; expectStatus?: number }[];
  /**
   * WebAuthn virtual authenticator. When set, the runner attaches a CDP
   * virtual authenticator to the context before steps run, so passkey
   * registrations/assertions go through the crawler's in-memory key
   * material instead of failing with NotAllowedError. See
   * https://chromedevtools.github.io/devtools-protocol/tot/WebAuthn/.
   */
  webauthn?: {
    enabled: boolean;
    protocol?: 'ctap2' | 'u2f';
    transport?: 'usb' | 'nfc' | 'ble' | 'internal';
    hasResidentKey?: boolean;
    hasUserVerification?: boolean;
    automaticPresenceSimulation?: boolean;
    isUserVerified?: boolean;
  };
  /**
   * Per-journey budget overrides. Default is "0 new console errors / 0 new
   * page errors / 0 new failed fetches / 0 new a11y / 0 newly broken
   * steps." Set to a non-zero value if a journey is expected to surface
   * a known-tolerated error (e.g., known third-party CSP warning).
   */
  budget?: {
    newConsoleErrors?: number;
    newPageErrors?: number;
    newFailedRequests?: number;
    newA11yViolations?: number;
    newlyBrokenSteps?: number;
  };
  /**
   * Allowlist for events that are EXPECTED on this journey and should
   * NOT trigger the regression budget. Each entry pins by kind / URL
   * substring / status / regex on text — preferring narrow matches so
   * you can't accidentally hide a real error class. Each entry should
   * include a `reason` so an operator reviewing this file later
   * understands why the noise was suppressed.
   *
   * Example: a TEST-seeded voter session has no voter row, so
   * `/api/voter/dashboard` returns 400 by design — that's not a bug,
   * but the crawler can't tell without an explicit allow.
   */
  expectedErrors?: Array<{
    kind?: 'console' | 'pageerror' | 'request-failed' | 'response-error' | 'csp-violation' | 'a11y-violation';
    level?: string;
    urlIncludes?: string;
    status?: number;
    textMatches?: string;
    reason?: string;
  }>;
  /**
   * localStorage / sessionStorage entries to seed before navigation. Uses
   * Playwright's `addInitScript` so the values are present on every page
   * load, not just the first navigation. Useful for dismissing first-time
   * popups (mission modal, walkthrough overlay) that gate access to the
   * UI under test. Keys are origin-scoped — same as a real browser visit.
   */
  seedStorage?: {
    localStorage?: Record<string, string>;
    sessionStorage?: Record<string, string>;
  };
}

export interface StepResult {
  step: Step;
  index: number;
  ok: boolean;
  durationMs: number;
  error?: string;
  screenshot?: string;
}

export async function runStep(page: Page, step: Step, timeout = 10_000): Promise<StepResult> {
  const start = Date.now();
  const out: StepResult = { step, index: 0, ok: true, durationMs: 0 };
  try {
    switch (step.kind) {
      case 'goto':
        // PlausiDen's SPA keeps WebSocket reconnection attempts + long-
        // polling open, so 'networkidle' never fires → 10s timeout on
        // every goto. Use 'domcontentloaded' (fires when the HTML is
        // parsed) which is what we actually want: the page is loaded,
        // subsequent steps wait for specific selectors anyway.
        await page.goto(step.url || '', { waitUntil: 'domcontentloaded', timeout: step.timeout || timeout });
        break;
      case 'click':
        if (!step.selector) throw new Error('click: missing selector');
        await page.click(step.selector, { timeout: step.timeout || timeout });
        break;
      case 'fill':
        if (!step.selector) throw new Error('fill: missing selector');
        await page.fill(step.selector, step.text || '', { timeout: step.timeout || timeout });
        break;
      case 'type':
        if (!step.selector) throw new Error('type: missing selector');
        await page.type(step.selector, step.text || '', { delay: 30 });
        break;
      case 'press':
        if (!step.key) throw new Error('press: missing key');
        if (step.selector) await page.press(step.selector, step.key);
        else await page.keyboard.press(step.key);
        break;
      case 'wait':
        await page.waitForTimeout(step.ms ?? 500);
        break;
      case 'waitForSelector':
        if (!step.selector) throw new Error('waitForSelector: missing selector');
        await page.waitForSelector(step.selector, { timeout: step.timeout || timeout });
        break;
      case 'screenshot':
        // handled by runner so it can track the filename
        break;
      case 'assertText':
        if (!step.selector) throw new Error('assertText: missing selector');
        const actual = await page.textContent(step.selector, { timeout: step.timeout || timeout });
        if (!actual?.includes(step.text || '')) {
          throw new Error(`assertText: "${step.text}" not found in ${step.selector}`);
        }
        break;
      case 'assertUrl': {
        // Pin "I got past the login form" by URL match. step.text is a
        // regex source string (anchor it with ^ / $ if you mean exact).
        if (!step.text) throw new Error('assertUrl: missing text (regex)');
        const re = new RegExp(step.text);
        // Wait briefly for client-side router transitions.
        const deadline = Date.now() + (step.timeout || timeout);
        let last = page.url();
        while (Date.now() < deadline) {
          last = page.url();
          if (re.test(last)) break;
          await page.waitForTimeout(150);
        }
        if (!re.test(last)) {
          throw new Error(`assertUrl: ${last} does not match /${step.text}/`);
        }
        break;
      }
      case 'assertQR': {
        // Find an <svg> matching the selector (qrcode.react renders one),
        // rasterize to canvas, decode with jsqr (injected from the runner),
        // and match the decoded payload against step.text (regex).
        // Catches the #268-class regression where the QR encoded raw
        // request_uri JSON instead of the openid4vp:// deep link.
        if (!step.selector) throw new Error('assertQR: missing selector');
        if (!step.text) throw new Error('assertQR: missing text (regex)');
        // jsqr is injected by main.ts into page context as window.__jsqr.
        // Step body runs in-page: serialize SVG → image → canvas → ImageData → jsqr.
        // Rasterize at 1024px so dense QR payloads (long openid4vp:// URIs ≈ 300-500 chars
        // encoded at ECC level M) decode reliably; 256px loses too many module pixels.
        const decoded: { ok: boolean; value?: string; error?: string } = await page.evaluate(
          async ([sel]: [string]) => {
            const svg = document.querySelector(sel) as SVGElement | null;
            if (!svg) return { ok: false, error: `no element matched ${sel}` };
            const clone = svg.cloneNode(true) as SVGElement;
            clone.setAttribute('width', '1024');
            clone.setAttribute('height', '1024');
            const xml = new XMLSerializer().serializeToString(clone);
            // Blob URL avoids long-data-URL parsing issues some renderers hit.
            const blob = new Blob([xml], { type: 'image/svg+xml' });
            const blobUrl = URL.createObjectURL(blob);
            const img = new Image();
            try {
              await new Promise<void>((resolve, reject) => {
                img.onload = () => resolve();
                img.onerror = () => reject(new Error('svg→img load failed'));
                img.src = blobUrl;
              });
            } finally {
              // free the blob URL whether load succeeded or failed
              try { URL.revokeObjectURL(blobUrl); } catch {}
            }
            const w = 1024, h = 1024;
            const c = document.createElement('canvas');
            c.width = w; c.height = h;
            const ctx = c.getContext('2d');
            if (!ctx) return { ok: false, error: 'canvas 2d unavailable' };
            ctx.fillStyle = '#fff';
            ctx.fillRect(0, 0, w, h);
            ctx.imageSmoothingEnabled = false;
            ctx.drawImage(img, 0, 0, w, h);
            const data = ctx.getImageData(0, 0, w, h);
            const jsqr = (window as any).__jsqr;
            if (!jsqr) return { ok: false, error: 'jsqr not injected' };
            // Attempt several scales — jsqr is sometimes finicky about module size.
            for (const size of [1024, 512, 768, 256]) {
              if (size === 1024) {
                const out = jsqr(data.data, w, h);
                if (out) return { ok: true, value: out.data };
                continue;
              }
              const c2 = document.createElement('canvas');
              c2.width = size; c2.height = size;
              const ctx2 = c2.getContext('2d');
              if (!ctx2) continue;
              ctx2.fillStyle = '#fff';
              ctx2.fillRect(0, 0, size, size);
              ctx2.imageSmoothingEnabled = false;
              ctx2.drawImage(c, 0, 0, size, size);
              const d2 = ctx2.getImageData(0, 0, size, size);
              const out = jsqr(d2.data, size, size);
              if (out) return { ok: true, value: out.data };
            }
            return { ok: false, error: 'jsqr decoded null at all tried sizes' };
          },
          [step.selector],
        );
        if (!decoded.ok) throw new Error(`assertQR: decode failed — ${decoded.error}`);
        const re = new RegExp(step.text);
        if (!re.test(decoded.value || '')) {
          throw new Error(`assertQR: decoded "${(decoded.value || '').slice(0, 80)}…" does not match /${step.text}/`);
        }
        break;
      }
      case 'scroll':
        await page.evaluate((dy: number) => window.scrollBy(0, dy), step.dy ?? 500);
        break;
      case 'reload':
        await page.reload({ waitUntil: 'domcontentloaded' });
        break;
    }
  } catch (e: any) {
    out.ok = false;
    out.error = String(e?.message || e);
  } finally {
    out.durationMs = Date.now() - start;
  }
  return out;
}
