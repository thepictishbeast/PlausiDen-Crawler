/**
 * outboundLinks.test.ts — pure-function tests for the outbound-link
 * safety detector (T76).
 */
import {
  detectOutboundLinkIssues,
  type OutboundLinksSnapshot,
  type CapturedOutboundLink,
} from './outboundLinks.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const baseSnap = (): OutboundLinksSnapshot => ({
  pageUrl: 'https://example.com/',
  pageOrigin: 'https://example.com',
  links: [],
});

const link = (
  over: Partial<CapturedOutboundLink> = {},
): CapturedOutboundLink => ({
  selector: 'body > a',
  href: 'https://other.example/',
  target: '',
  rel: [],
  outbound: true,
  ...over,
});

// 1. Same-origin _blank — not even captured by the snapshot, but
//    defensive: if it sneaks through with outbound=false, no findings
//    related to _blank should fire.
{
  const s = baseSnap();
  s.links.push(link({ outbound: false, target: '_blank', rel: [] }));
  const f = detectOutboundLinkIssues(s);
  assert(
    !f.some((x) => x.kind === 'link.tabnab-vulnerable'),
    'same-origin _blank not flagged',
    JSON.stringify(f),
  );
}

// 2. Outbound _blank with no rel → strict tabnab-vulnerable AND
//    warn no-noreferrer (independent failure modes).
{
  const s = baseSnap();
  s.links.push(link({ target: '_blank', rel: [] }));
  const f = detectOutboundLinkIssues(s);
  assert(
    f.some((x) => x.kind === 'link.tabnab-vulnerable' && x.severity === 'strict'),
    'tabnab-vulnerable fires strict',
    JSON.stringify(f),
  );
  assert(
    f.some((x) => x.kind === 'link.outbound-no-noreferrer'),
    'no-noreferrer also fires (independent)',
    JSON.stringify(f),
  );
}

// 3. Outbound _blank WITH rel="noopener" → no tabnab finding,
//    but noreferrer warn still fires.
{
  const s = baseSnap();
  s.links.push(link({ target: '_blank', rel: ['noopener'] }));
  const f = detectOutboundLinkIssues(s);
  assert(
    !f.some((x) => x.kind === 'link.tabnab-vulnerable'),
    'noopener suppresses tabnab',
    JSON.stringify(f),
  );
  assert(
    f.some((x) => x.kind === 'link.outbound-no-noreferrer'),
    'noopener does not also add noreferrer',
    JSON.stringify(f),
  );
}

// 4. Outbound _blank WITH rel="noopener noreferrer" → clean.
{
  const s = baseSnap();
  s.links.push(link({ target: '_blank', rel: ['noopener', 'noreferrer'] }));
  const f = detectOutboundLinkIssues(s);
  assert(f.length === 0, 'noopener+noreferrer is clean', JSON.stringify(f));
}

// 5. Outbound _blank with explicit rel="opener" → strict opener-
//    explicit. tabnab does NOT also fire (opener-explicit is the
//    more specific finding for the same root cause).
{
  const s = baseSnap();
  s.links.push(link({ target: '_blank', rel: ['opener'] }));
  const f = detectOutboundLinkIssues(s);
  assert(
    f.some((x) => x.kind === 'link.opener-explicit' && x.severity === 'strict'),
    'opener-explicit fires strict',
    JSON.stringify(f),
  );
  assert(
    !f.some((x) => x.kind === 'link.tabnab-vulnerable'),
    'tabnab suppressed when opener-explicit fires',
    JSON.stringify(f),
  );
}

// 6. Same-tab outbound link (no target) → only the noreferrer
//    warn (no tabnab — there's no other tab to navigate).
{
  const s = baseSnap();
  s.links.push(link({ target: '', rel: [] }));
  const f = detectOutboundLinkIssues(s);
  assert(
    !f.some((x) => x.kind === 'link.tabnab-vulnerable'),
    'same-tab outbound has no tabnab',
    JSON.stringify(f),
  );
  assert(
    f.some((x) => x.kind === 'link.outbound-no-noreferrer'),
    'same-tab outbound still warns on noreferrer',
    JSON.stringify(f),
  );
}

// 7. Same-tab outbound with rel="noreferrer" → fully clean.
{
  const s = baseSnap();
  s.links.push(link({ target: '', rel: ['noreferrer'] }));
  const f = detectOutboundLinkIssues(s);
  assert(f.length === 0, 'same-tab + noreferrer is clean', JSON.stringify(f));
}

// 8. Multiple offenders aggregate correctly.
{
  const s = baseSnap();
  for (let i = 0; i < 5; i++) {
    s.links.push(
      link({ selector: `body > a:nth-of-type(${i + 1})`, target: '_blank', rel: [] }),
    );
  }
  const f = detectOutboundLinkIssues(s);
  const tabnab = f.find((x) => x.kind === 'link.tabnab-vulnerable');
  assert(
    !!tabnab && (tabnab.evidence.count as number) === 5,
    '5 vulnerable links aggregate to count=5',
    JSON.stringify(f),
  );
}

// 9. Examples capped at 5 even with 12 offenders.
{
  const s = baseSnap();
  for (let i = 0; i < 12; i++) {
    s.links.push(
      link({ selector: `body > a:nth-of-type(${i + 1})`, target: '_blank', rel: [] }),
    );
  }
  const f = detectOutboundLinkIssues(s);
  const tabnab = f.find((x) => x.kind === 'link.tabnab-vulnerable');
  const examples = tabnab?.evidence.examples as string[];
  assert(examples.length === 5, 'examples capped at 5', JSON.stringify(examples));
  assert(
    (tabnab?.evidence.count as number) === 12,
    'count still reflects all 12',
    JSON.stringify(tabnab),
  );
}

// 10. Mixed: 1 vulnerable, 1 explicit-opener, 1 clean.
{
  const s = baseSnap();
  s.links.push(link({ selector: 'a:nth-of-type(1)', target: '_blank', rel: [] }));
  s.links.push(link({ selector: 'a:nth-of-type(2)', target: '_blank', rel: ['opener'] }));
  s.links.push(link({ selector: 'a:nth-of-type(3)', target: '_blank', rel: ['noopener', 'noreferrer'] }));
  const f = detectOutboundLinkIssues(s);
  assert(
    f.some((x) => x.kind === 'link.tabnab-vulnerable') &&
      f.some((x) => x.kind === 'link.opener-explicit'),
    'mixed: both findings fire',
    JSON.stringify(f),
  );
}

console.log('\n=== outboundLinks.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
