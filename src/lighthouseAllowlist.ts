/**
 * lighthouseAllowlist.ts — the frozen set of Lighthouse audit ids this
 * repo is willing to ingest, and the reason each one is net-new.
 *
 * ## Why an allowlist rather than the whole LHR
 *
 * Lighthouse 13.4.1 emits **160** audits. Most of them are already
 * answered here, by one of the 101 detectors in
 * `crates/crawler-detectors/src/` or by a dependency this repo already
 * drives. Shipping the whole LHR into analytics.plausiden.com would
 * double-count:
 *
 *   * the entire Accessibility category IS axe-core, and axe-core is
 *     already injected at `src/audit.ts` (`require_.resolve(
 *     'axe-core/axe.min.js')`), so every `aria-*`, `color-contrast`,
 *     `label`, `heading-order` finding would arrive twice;
 *   * `largest-contentful-paint`, `cumulative-layout-shift`,
 *     `interaction-to-next-paint`, `first-contentful-paint` and
 *     `total-blocking-time` duplicate the `web-vitals` dependency and
 *     `crates/crawler-detectors/src/web_vitals.rs`;
 *   * `long-tasks` / `main-thread-tasks` duplicate `long_tasks.rs`.
 *
 * A double-counted finding is worse than a missing one: it makes the
 * same defect look like two defects and inflates every trend built on
 * top of it.
 *
 * ## The ids below were derived from a REAL run, not from the docs
 *
 * `docs/LIGHTHOUSE_PARITY.md` was written against the Lighthouse v11
 * audit list and is a trap. Lighthouse 13 renamed or replaced most of
 * the byte-weight audits with "insight" audits, so the ids everyone
 * remembers **do not exist any more**. Verified absent from a live
 * 13.4.1 run against https://plausiden.com/ on 2026-09-08:
 *
 *   render-blocking-resources   duplicated-javascript
 *   uses-responsive-images      legacy-javascript
 *   modern-image-formats        critical-request-chains
 *   uses-optimized-images       uses-http2
 *   third-party-summary         efficient-animated-content
 *   font-size
 *
 * Coding to any of those would have produced an adapter that silently
 * emitted nothing while reporting a clean run. That is why
 * `assertAllowlistPresent` exists: an id in this table that Lighthouse
 * does not emit is a LOUD failure, never a quiet zero.
 *
 * ## Excluded on purpose, with the audit that covers it
 *
 * Anything not in this table is dropped at the adapter boundary. The
 * near-misses, so nobody re-adds them:
 *
 *   render-blocking-insight        -> render_blocking_resources.rs
 *   image-delivery-insight         -> modern_image_formats.rs
 *                                     (its own header: "Lighthouse
 *                                     modern-image-formats +
 *                                     uses-optimized-images audits")
 *   unsized-images,
 *   image-size-responsive,
 *   image-aspect-ratio             -> image_dimensions.rs
 *   doctype, charset               -> doctype_charset.rs
 *   robots-txt, is-crawlable       -> robots_txt.rs
 *   canonical                      -> canonical_url.rs
 *   meta-description               -> meta_description.rs
 *   document-title                 -> doc_title.rs
 *   hreflang                       -> hreflang.rs
 *   font-display-insight           -> font_loading.rs
 *   dom-size-insight               -> dom_size.rs
 *   cache-insight                  -> cache_control.rs
 *   is-on-https                    -> mixed_content.rs
 *   csp-xss                        -> content_security_policy.rs
 *   trusted-types-xss              -> trusted_types_runtime.rs
 *   has-hsts                       -> hsts.rs
 *   clickjacking-mitigation        -> x_frame_options.rs
 *   origin-isolation               -> origin_agent_cluster.rs, coop.rs
 *   third-party-cookies            -> cookie_security.rs
 *   meta-viewport                  -> viewport_meta.rs
 *   meta-refresh                   -> meta_refresh.rs
 *   forced-reflow-insight          -> layout_thrash.rs
 *   long-tasks, main-thread-tasks  -> long_tasks.rs
 *   bypass, skip-link              -> skip_link.rs
 *   link-text                      -> link_text.rs
 *   the ~60 aria, label and
 *   colour-contrast audits         -> axe-core (src/audit.ts)
 *   the vitals + metrics audits    -> web_vitals.rs, web-vitals dep
 */

/** Why an id is here, and what it was checked against. */
export interface AllowedAudit {
  id: string;
  /** What it measures, in the words a reader of the panel needs. */
  what: string;
  /**
   * The detector this was checked against. `null` means no detector in
   * `crates/crawler-detectors/src/` addresses the question at all.
   */
  adjacentDetector: string | null;
}

/**
 * Fourteen ids. Every one verified PRESENT and non-erroring in a live
 * Lighthouse 13.4.1 run, and verified to have no detector answering the
 * same question.
 */
export const ALLOWED_AUDITS: readonly AllowedAudit[] = Object.freeze([
  // ── recovered by pinning the audit unit to Node 22 (2026-09-09) ──
  //
  // These three returned scoreDisplayMode "error" on EVERY run under
  // Node 20: they use ES2025 iterator helpers (.values().flatMap,
  // .values().reduce, .values().find) that Node 20 does not implement,
  // so Lighthouse caught the TypeError and scored them as errors.
  // Measured on this host against a live page:
  //   node 20.19.2 -> error, error, error
  //   node 22.23.2 -> metricSavings, metricSavings, numeric
  {
    id: 'duplicated-javascript-insight',
    what: 'The same module shipped more than once across bundles.',
    adjacentDetector: null,
  },
  {
    id: 'legacy-javascript-insight',
    what: 'Transpiled polyfills served to browsers that do not need them.',
    adjacentDetector: null,
  },
  {
    id: 'third-parties-insight',
    what: 'What third-party origins cost the page in main-thread time.',
    adjacentDetector: null,
  },
  {
    id: 'speed-index',
    what: 'How quickly the page paints its content, as one number.',
    adjacentDetector: null,
  },
  {
    id: 'unused-css-rules',
    what: 'CSS bytes shipped and never matched, measured by CDP coverage.',
    // Kept, but FLAGGED. css_health.rs counts declared braces against
    // applied rules, which is a proxy for the same waste by a different
    // method. A panel showing both must reconcile them or one CSS
    // problem reads as two independent findings.
    adjacentDetector: 'css_health.rs (brace-count proxy, different method)',
  },
  {
    id: 'unused-javascript',
    what: 'JavaScript bytes parsed and never executed.',
    adjacentDetector: null,
  },
  {
    id: 'unminified-css',
    what: 'Stylesheets shipped with the whitespace and comments still in.',
    adjacentDetector: null,
  },
  {
    id: 'unminified-javascript',
    what: 'Scripts shipped with the whitespace and comments still in.',
    adjacentDetector: null,
  },
  {
    id: 'total-byte-weight',
    what: 'Total transfer size of everything the page pulled.',
    adjacentDetector: null,
  },
  {
    id: 'server-response-time',
    what: 'How long the origin took to hand over the root document.',
    adjacentDetector: null,
  },
  {
    id: 'network-rtt',
    what: 'Round-trip time to each origin the page talked to.',
    adjacentDetector: null,
  },
  {
    id: 'network-server-latency',
    what: 'Per-origin backend latency, separated from the RTT.',
    adjacentDetector: null,
  },
  {
    id: 'modern-http-insight',
    what: 'Requests still being served over HTTP/1.1.',
    // The Lighthouse 13 replacement for the `uses-http2` id that
    // docs/LIGHTHOUSE_PARITY.md still names. No detector reads the
    // negotiated protocol.
    adjacentDetector: null,
  },
  {
    id: 'network-dependency-tree-insight',
    what: 'The longest chain of requests that must finish before paint.',
    // The Lighthouse 13 replacement for `critical-request-chains`.
    // Adjacent to render_blocking_resources.rs, which asks a narrower
    // question (is this <head> tag deferred), not how deep the chain is.
    adjacentDetector: 'render_blocking_resources.rs (adjacent, narrower)',
  },
  {
    id: 'bootup-time',
    what: 'Parse, compile and execute time attributed to each script.',
    // long_tasks.rs counts main-thread tasks over 50ms and sums TBT. It
    // has no per-script attribution, which is the whole point here.
    adjacentDetector: 'long_tasks.rs (no per-script attribution)',
  },
  {
    id: 'mainthread-work-breakdown',
    what: 'Where main-thread time went, by category.',
    adjacentDetector: 'long_tasks.rs (counts tasks, not categories)',
  },
  {
    id: 'valid-source-maps',
    what: 'Shipped bundles whose source maps are missing or broken.',
    adjacentDetector: null,
  },
]);

/**
 * Ids that ARE net-new but that this host cannot collect, and why.
 *
 * Empty since 2026-09-09. Kept because the category is real and will
 * recur: an audit that errors on every run must NOT be allowlisted. An
 * allowlisted id that always errors pins the run state to `unusable`
 * forever, and a check that is red on ordinary days is not read on the
 * day it is right. Record such an id here instead, with the reason and
 * the condition that would clear it — as the Node 20 entries below did
 * until the audit unit was pinned to Node 22.
 */
// Empty since 2026-09-09: the estate now runs these under Node 22.
//
// On Node 20 all three returned scoreDisplayMode "error" on every run —
// they use ES2025 iterator helpers (`.values().flatMap`, `.values().reduce`,
// `.values().find`) that Node 20 does not implement, so Lighthouse caught
// the TypeError and scored them as errors. Allowlisting them there would
// have pinned every run to unusable.
//
// Verified on this host against a live page, both versions:
//   node 20.19.2 -> error, error, error
//   node 22.23.2 -> metricSavings, metricSavings, numeric
//
// The uxaudit unit pins /opt/node22 explicitly. If that pin is ever
// removed, these three must come back out of ALLOWED_IDS or every run
// reports three errors.
export const BLOCKED_BY_NODE_VERSION: readonly string[] = Object.freeze([]);

export const ALLOWED_IDS: readonly string[] = Object.freeze(
  ALLOWED_AUDITS.map((a) => a.id),
);

/** Fast membership test for the adapter's drop-everything-else rule. */
export const ALLOWED_ID_SET: ReadonlySet<string> = new Set(ALLOWED_IDS);
