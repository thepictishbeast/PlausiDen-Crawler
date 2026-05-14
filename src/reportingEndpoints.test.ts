/**
 * reportingEndpoints.test.ts — Reporting API endpoint detector
 * tests. T76 cycle 31.
 */
import {
  buildReportingEndpointsSnapshot,
  detectReportingEndpointsIssues,
} from './reportingEndpoints.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const PAGE = 'https://example.com/';

// 1. No headers → no-endpoints warn.
{
  const s = buildReportingEndpointsSnapshot(PAGE, {});
  const f = detectReportingEndpointsIssues(s);
  assert(
    f.length === 1 && f[0].kind === 'reporting.no-endpoints',
    'no headers → no-endpoints',
    JSON.stringify(f),
  );
}

// 2. Localhost exempt.
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot('https://localhost:3000/', {}));
  assert(f.length === 0, 'localhost exempt', JSON.stringify(f));
}

// 3. Modern Reporting-Endpoints set → no findings.
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'reporting-endpoints': 'csp-default="https://example.com/csp"',
  }));
  assert(f.length === 0, 'modern Reporting-Endpoints clean', JSON.stringify(f));
}

// 4. Multiple endpoints parsed.
{
  const s = buildReportingEndpointsSnapshot(PAGE, {
    'reporting-endpoints': 'csp-default="https://example.com/csp", crash-reports="https://example.com/crashes"',
  });
  assert(s.endpoints.length === 2, 'two endpoints parsed', JSON.stringify(s));
  assert(s.endpoints[0].name === 'csp-default', 'first endpoint name', JSON.stringify(s));
  assert(s.endpoints[1].url === 'https://example.com/crashes', 'second endpoint url', JSON.stringify(s));
}

// 5. Report-To only (legacy) → report-to-only warn.
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'report-to': '{"group":"csp","max_age":86400,"endpoints":[{"url":"https://example.com/csp"}]}',
  }));
  assert(
    f.some((x) => x.kind === 'reporting.report-to-only'),
    'Report-To only → warn',
    JSON.stringify(f),
  );
}

// 6. Both Reporting-Endpoints AND Report-To set → no warn (modern wins).
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'reporting-endpoints': 'csp-default="https://example.com/csp"',
    'report-to': '{"group":"csp","max_age":86400,"endpoints":[{"url":"https://example.com/csp"}]}',
  }));
  assert(f.length === 0, 'both modern + legacy clean', JSON.stringify(f));
}

// 7. Reporting-Endpoints garbage → invalid warn + no-endpoints.
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'reporting-endpoints': '   ,   ,   ',
  }));
  assert(
    f.some((x) => x.kind === 'reporting.invalid'),
    'garbage → invalid warn',
    JSON.stringify(f),
  );
  // The detector ALSO reports no-endpoints since no parsed entry → effectively no endpoint configured.
  assert(
    f.some((x) => x.kind === 'reporting.no-endpoints'),
    'garbage also fires no-endpoints (no parsed entry)',
    JSON.stringify(f),
  );
}

// 8. CSP with report-uri but no Reporting-Endpoints → csp-report-uri-no-endpoints warn.
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'content-security-policy': "default-src 'self'; report-uri /csp-report",
  }));
  assert(
    f.some((x) => x.kind === 'reporting.csp-report-uri-no-endpoints'),
    'CSP with report-uri + no endpoints → warn',
    JSON.stringify(f),
  );
}

// 9. CSP with report-to but no Reporting-Endpoints → also warn.
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'content-security-policy': "default-src 'self'; report-to csp-default",
  }));
  assert(
    f.some((x) => x.kind === 'reporting.csp-report-uri-no-endpoints'),
    'CSP with report-to + no endpoints → warn',
    JSON.stringify(f),
  );
}

// 10. CSP with report-to + Reporting-Endpoints set → no orphan warn.
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'content-security-policy': "default-src 'self'; report-to csp-default",
    'reporting-endpoints': 'csp-default="https://example.com/csp"',
  }));
  assert(
    !f.some((x) => x.kind === 'reporting.csp-report-uri-no-endpoints'),
    'CSP with report-to + endpoints clean',
    JSON.stringify(f),
  );
}

// 11. Header name case-insensitive.
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'Reporting-Endpoints': 'csp-default="https://example.com/csp"',
  }));
  assert(f.length === 0, 'header name case-insensitive', JSON.stringify(f));
}

// 12. Endpoint with no quotes (some servers omit) - still parses.
{
  const s = buildReportingEndpointsSnapshot(PAGE, {
    'reporting-endpoints': 'csp-default=https://example.com/csp',
  });
  assert(
    s.endpoints.length === 1 && s.endpoints[0].url === 'https://example.com/csp',
    'unquoted URL parsed',
    JSON.stringify(s),
  );
}

// 13. Empty Reporting-Endpoints header → invalid + no-endpoints (treated as garbage).
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'reporting-endpoints': '',
  }));
  // Empty string isn't "unparseable" by our heuristic (it's just empty),
  // so we expect ONLY no-endpoints. But the snapshot's endpoints array
  // is empty, so we also expect the no-endpoints finding.
  assert(
    f.some((x) => x.kind === 'reporting.no-endpoints'),
    'empty value → no-endpoints',
    JSON.stringify(f),
  );
}

// 14. Whitespace tolerated in endpoint syntax.
{
  const s = buildReportingEndpointsSnapshot(PAGE, {
    'reporting-endpoints': '  csp-default  =  "https://example.com/csp"  ,  crash  =  "https://example.com/crash"  ',
  });
  assert(s.endpoints.length === 2, 'whitespace tolerated', JSON.stringify(s));
}

// 15. cspMentionsReporting case-insensitive (CSP directive names are case-insensitive).
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'content-security-policy': "default-src 'self'; REPORT-URI /csp-report",
  }));
  assert(
    f.some((x) => x.kind === 'reporting.csp-report-uri-no-endpoints'),
    'CSP REPORT-URI uppercase still flagged',
    JSON.stringify(f),
  );
}

// 16. T76 cycle 64: CSP `report-to <name>` references a group
// not declared in Reporting-Endpoints → undeclared warn.
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'reporting-endpoints': 'csp-violations="/csp"',
    'content-security-policy': "default-src 'self'; report-to default",
  }));
  assert(
    f.some((x) => x.kind === 'reporting.csp-group-undeclared'),
    'CSP report-to references undeclared group → warn',
    JSON.stringify(f),
  );
}

// 17. T76 cycle 64: CSP `report-to` matches declared group → no
// undeclared warn (but may fire orphan if other groups exist).
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'reporting-endpoints': 'default="/reports"',
    'content-security-policy': "default-src 'self'; report-to default",
  }));
  assert(
    !f.some((x) => x.kind === 'reporting.csp-group-undeclared'),
    'matching CSP group + endpoint → no undeclared warn',
    JSON.stringify(f),
  );
}

// 18. T76 cycle 64: orphan endpoint declaration → warn.
// Reporting-Endpoints declares 2 groups but CSP only references 1.
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'reporting-endpoints': 'default="/reports", crashes="/crash"',
    'content-security-policy': "default-src 'self'; report-to default",
  }));
  assert(
    f.some((x) => x.kind === 'reporting.endpoint-orphan'),
    'orphan endpoint declaration → warn',
    JSON.stringify(f),
  );
}

// 19. T76 cycle 64: non-HTTPS endpoint → warn.
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'reporting-endpoints': 'default="http://example.com/reports"',
    'content-security-policy': "default-src 'self'; report-to default",
  }));
  assert(
    f.some((x) => x.kind === 'reporting.endpoint-not-https'),
    'http:// endpoint URL → not-https warn',
    JSON.stringify(f),
  );
}

// 20. T76 cycle 64: same-origin path endpoint → no not-https
// warn (path inherits the page origin's scheme).
{
  const f = detectReportingEndpointsIssues(buildReportingEndpointsSnapshot(PAGE, {
    'reporting-endpoints': 'default="/reports"',
    'content-security-policy': "default-src 'self'; report-to default",
  }));
  assert(
    !f.some((x) => x.kind === 'reporting.endpoint-not-https'),
    'same-origin path endpoint → no not-https warn',
    JSON.stringify(f),
  );
}

console.log('\n=== reportingEndpoints.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
