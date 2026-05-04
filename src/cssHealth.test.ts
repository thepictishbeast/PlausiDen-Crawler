/**
 * cssHealth.test.ts — pure-function tests for detectCSSHealthIssues.
 *
 * Run with: npx tsx src/cssHealth.test.ts
 *
 * No browser, no I/O — feeds hand-crafted snapshots through the
 * detector. Each scenario is a "what a real broken site would look
 * like to a visitor" snapshot.
 */
import { detectCSSHealthIssues, type CSSHealthSnapshot } from './cssHealth.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];

function assert(cond: boolean, name: string, reason: string): void {
  if (cond) PASSED.push(name);
  else FAILED.push({ name, reason });
}

function uaDefaultBody(): CSSHealthSnapshot['computedBody'] {
  return {
    backgroundColor: 'rgba(0, 0, 0, 0)',
    color: 'rgb(0, 0, 0)',
    fontFamily: 'Times',
    fontSize: '16px',
    margin: '8px',
  };
}

function styledBody(): CSSHealthSnapshot['computedBody'] {
  return {
    backgroundColor: 'rgb(15, 23, 42)',
    color: 'rgb(248, 250, 252)',
    fontFamily: '"Inter", system-ui, sans-serif',
    fontSize: '16px',
    margin: '0px',
  };
}

// --- scenario 1: working CSS — must produce zero findings ---
{
  const snap: CSSHealthSnapshot = {
    pageUrl: 'http://test/working',
    declaredSheets: [
      { url: 'http://test/style.css', status: 200, contentType: 'text/css', bodyBytes: 5000, declaredBraceCount: 80, declaredCloseBraceCount: 80, fromNetwork: true, errorText: null },
    ],
    inlineStyleBlockCount: 0,
    computedBody: styledBody(),
    computedHtml: { backgroundColor: 'rgb(15, 23, 42)' },
    bodyVisibleTextLength: 200,
    appliedRuleCountEstimate: 80,
  };
  const findings = detectCSSHealthIssues(snap);
  assert(
    findings.length === 0,
    'working-css produces no findings',
    `expected 0 findings, got ${findings.length}: ${JSON.stringify(findings)}`,
  );
}

// --- scenario 2: missing CSS — declared sheet 404 ---
{
  const snap: CSSHealthSnapshot = {
    pageUrl: 'http://test/missing',
    declaredSheets: [
      { url: 'http://test/missing.css', status: 404, contentType: 'text/html', bodyBytes: 0, declaredBraceCount: null, declaredCloseBraceCount: null, fromNetwork: true, errorText: null },
    ],
    inlineStyleBlockCount: 0,
    computedBody: uaDefaultBody(),
    computedHtml: { backgroundColor: 'rgba(0, 0, 0, 0)' },
    bodyVisibleTextLength: 200,
    appliedRuleCountEstimate: 0,
  };
  const findings = detectCSSHealthIssues(snap);
  assert(
    findings.some((f) => f.kind === 'css.all-sheets-failed-network'),
    'missing-css triggers all-sheets-failed-network',
    `findings: ${JSON.stringify(findings.map((f) => f.kind))}`,
  );
}

// --- scenario 3: empty CSS file ---
{
  const snap: CSSHealthSnapshot = {
    pageUrl: 'http://test/empty',
    declaredSheets: [
      { url: 'http://test/empty.css', status: 200, contentType: 'text/css', bodyBytes: 0, declaredBraceCount: 0, declaredCloseBraceCount: 0, fromNetwork: true, errorText: null },
    ],
    inlineStyleBlockCount: 0,
    computedBody: uaDefaultBody(),
    computedHtml: { backgroundColor: 'rgba(0, 0, 0, 0)' },
    bodyVisibleTextLength: 200,
    appliedRuleCountEstimate: 0,
  };
  const findings = detectCSSHealthIssues(snap);
  assert(
    findings.some((f) => f.kind === 'css.empty-or-tiny-body'),
    'empty-css triggers empty-or-tiny-body',
    `findings: ${JSON.stringify(findings.map((f) => f.kind))}`,
  );
}

// --- scenario 4: wrong MIME (server returns text/html for .css) ---
{
  const snap: CSSHealthSnapshot = {
    pageUrl: 'http://test/wrongmime',
    declaredSheets: [
      { url: 'http://test/style.css', status: 200, contentType: 'text/html; charset=utf-8', bodyBytes: 5000, declaredBraceCount: 0, declaredCloseBraceCount: 0, fromNetwork: true, errorText: null },
    ],
    inlineStyleBlockCount: 0,
    computedBody: uaDefaultBody(),
    computedHtml: { backgroundColor: 'rgba(0, 0, 0, 0)' },
    bodyVisibleTextLength: 200,
    appliedRuleCountEstimate: 0,
  };
  const findings = detectCSSHealthIssues(snap);
  assert(
    findings.some((f) => f.kind === 'css.wrong-mime'),
    'wrong-mime triggers wrong-mime finding',
    `findings: ${JSON.stringify(findings.map((f) => f.kind))}`,
  );
}

// --- scenario 5: served-but-not-applied (parse error swallows everything) ---
{
  const snap: CSSHealthSnapshot = {
    pageUrl: 'http://test/parsefail',
    declaredSheets: [
      { url: 'http://test/parsefail.css', status: 200, contentType: 'text/css', bodyBytes: 5000, declaredBraceCount: 1, declaredCloseBraceCount: 5, fromNetwork: true, errorText: null },
    ],
    inlineStyleBlockCount: 0,
    computedBody: uaDefaultBody(),
    computedHtml: { backgroundColor: 'rgba(0, 0, 0, 0)' },
    bodyVisibleTextLength: 200,
    appliedRuleCountEstimate: 1,
  };
  const findings = detectCSSHealthIssues(snap);
  assert(
    findings.some((f) => f.kind === 'css.served-but-not-applied' || f.kind === 'css.applied-rule-count-anomaly'),
    'parsefail triggers served-but-not-applied or applied-rule-count-anomaly',
    `findings: ${JSON.stringify(findings.map((f) => f.kind))}`,
  );
}

// --- scenario 6: zero stylesheets at all — warn-only ---
{
  const snap: CSSHealthSnapshot = {
    pageUrl: 'http://test/no-css',
    declaredSheets: [],
    inlineStyleBlockCount: 0,
    computedBody: uaDefaultBody(),
    computedHtml: { backgroundColor: 'rgba(0, 0, 0, 0)' },
    bodyVisibleTextLength: 50,
    appliedRuleCountEstimate: 0,
  };
  const findings = detectCSSHealthIssues(snap);
  assert(
    findings.some((f) => f.kind === 'css.no-stylesheets-declared' && f.severity === 'warn'),
    'no-css produces warn-severity finding',
    `findings: ${JSON.stringify(findings)}`,
  );
}

// --- scenario 7: partial failure — some sheets OK, some 404 ---
{
  const snap: CSSHealthSnapshot = {
    pageUrl: 'http://test/partial',
    declaredSheets: [
      { url: 'http://test/ok.css', status: 200, contentType: 'text/css', bodyBytes: 5000, declaredBraceCount: 80, declaredCloseBraceCount: 80, fromNetwork: true, errorText: null },
      { url: 'http://test/404.css', status: 404, contentType: 'text/html', bodyBytes: 0, declaredBraceCount: null, declaredCloseBraceCount: null, fromNetwork: true, errorText: null },
    ],
    inlineStyleBlockCount: 0,
    computedBody: styledBody(),
    computedHtml: { backgroundColor: 'rgb(15, 23, 42)' },
    bodyVisibleTextLength: 200,
    appliedRuleCountEstimate: 80,
  };
  const findings = detectCSSHealthIssues(snap);
  assert(
    findings.some((f) => f.kind === 'css.some-sheets-failed-network'),
    'partial failure triggers some-sheets-failed-network',
    `findings: ${JSON.stringify(findings.map((f) => f.kind))}`,
  );
}

// --- summary ---
console.log(`\n=== cssHealth.test.ts ===`);
console.log(`PASSED: ${PASSED.length}`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED: ${FAILED.length}`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}\n    ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
