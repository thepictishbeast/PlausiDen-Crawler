/**
 * autocomplete.test.ts — pure-function tests for the autocomplete
 * detector (T76).
 */
import {
  detectAutocompleteIssues,
  type AutocompleteSnapshot,
  type CapturedAutocompleteField,
} from './autocomplete.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const baseSnap = (): AutocompleteSnapshot => ({
  pageUrl: 'http://t/',
  fields: [],
});

const field = (
  over: Partial<CapturedAutocompleteField> = {},
): CapturedAutocompleteField => ({
  selector: 'body > form > input',
  type: 'text',
  name: 'comment',
  id: 'comment',
  autocomplete: '',
  hasAutocomplete: false,
  accessibleName: 'Your comment',
  ...over,
});

// 1. Clean — generic non-PII text input without autocomplete: no findings.
{
  const s = baseSnap();
  s.fields.push(field());
  const f = detectAutocompleteIssues(s);
  assert(f.length === 0, 'generic text field — no findings', JSON.stringify(f));
}

// 2. Email type without autocomplete → strict missing-credentials.
{
  const s = baseSnap();
  s.fields.push(field({ type: 'email', name: 'email', id: 'email', accessibleName: 'Email' }));
  const f = detectAutocompleteIssues(s);
  assert(
    f.some((x) => x.kind === 'autocomplete.missing-credentials' && x.severity === 'strict'),
    'email field without autocomplete → strict',
    JSON.stringify(f),
  );
}

// 3. Password type without autocomplete → strict.
{
  const s = baseSnap();
  s.fields.push(field({ type: 'password', name: 'pw', id: 'pw', accessibleName: 'Password' }));
  const f = detectAutocompleteIssues(s);
  assert(
    f.some((x) => x.kind === 'autocomplete.missing-credentials'),
    'password field flagged strict',
    JSON.stringify(f),
  );
}

// 4. Field named 'username' without autocomplete → strict.
{
  const s = baseSnap();
  s.fields.push(field({ name: 'username', id: 'username', accessibleName: 'Username' }));
  const f = detectAutocompleteIssues(s);
  assert(
    f.some((x) => x.kind === 'autocomplete.missing-credentials'),
    'username field flagged strict',
    JSON.stringify(f),
  );
}

// 5. PII field (phone) without autocomplete → warn missing-pii.
{
  const s = baseSnap();
  s.fields.push(field({ type: 'tel', name: 'phone', accessibleName: 'Phone' }));
  const f = detectAutocompleteIssues(s);
  assert(
    f.some((x) => x.kind === 'autocomplete.missing-pii' && x.severity === 'warn'),
    'tel field → warn missing-pii',
    JSON.stringify(f),
  );
}

// 6. Address-line1 field without autocomplete → warn.
{
  const s = baseSnap();
  s.fields.push(field({ name: 'street_address', accessibleName: 'Street address' }));
  const f = detectAutocompleteIssues(s);
  assert(
    f.some((x) => x.kind === 'autocomplete.missing-pii'),
    'street address → warn',
    JSON.stringify(f),
  );
}

// 7. Email field WITH autocomplete="email" → no findings.
{
  const s = baseSnap();
  s.fields.push(
    field({ type: 'email', name: 'email', autocomplete: 'email', hasAutocomplete: true }),
  );
  const f = detectAutocompleteIssues(s);
  assert(f.length === 0, 'autocomplete=email is clean', JSON.stringify(f));
}

// 8. autocomplete="off" is valid (developer opted out intentionally).
{
  const s = baseSnap();
  s.fields.push(
    field({ type: 'email', name: 'email', autocomplete: 'off', hasAutocomplete: true }),
  );
  const f = detectAutocompleteIssues(s);
  assert(f.length === 0, 'autocomplete=off is accepted', JSON.stringify(f));
}

// 9. autocomplete="bogus" → warn invalid-token.
{
  const s = baseSnap();
  s.fields.push(
    field({ name: 'comment', autocomplete: 'bogus', hasAutocomplete: true }),
  );
  const f = detectAutocompleteIssues(s);
  assert(
    f.some((x) => x.kind === 'autocomplete.invalid-token' && x.severity === 'warn'),
    'invalid token → warn',
    JSON.stringify(f),
  );
}

// 10. Multi-token "shipping street-address" → valid.
{
  const s = baseSnap();
  s.fields.push(
    field({ name: 'addr', autocomplete: 'shipping street-address', hasAutocomplete: true }),
  );
  const f = detectAutocompleteIssues(s);
  assert(f.length === 0, 'multi-token shipping address is valid', JSON.stringify(f));
}

// 11. section-* prefix accepted: "section-billing cc-number".
{
  const s = baseSnap();
  s.fields.push(
    field({ name: 'cc', autocomplete: 'section-billing cc-number', hasAutocomplete: true }),
  );
  const f = detectAutocompleteIssues(s);
  assert(f.length === 0, 'section-* prefix accepted', JSON.stringify(f));
}

// 12. Aggregation: multiple credential fields collapse to 1 finding.
{
  const s = baseSnap();
  s.fields.push(field({ type: 'email', name: 'email1', accessibleName: 'Email' }));
  s.fields.push(field({ type: 'password', name: 'pw1', accessibleName: 'Password' }));
  s.fields.push(field({ type: 'password', name: 'pw2', accessibleName: 'Confirm password' }));
  const f = detectAutocompleteIssues(s);
  const cred = f.find((x) => x.kind === 'autocomplete.missing-credentials');
  assert(
    !!cred && (cred.evidence.count as number) === 3,
    '3 credential fields → 1 finding count=3',
    JSON.stringify(cred),
  );
}

// 13. Generic non-PII fields: textarea labeled "Comments" without
//     autocomplete is fine.
{
  const s = baseSnap();
  s.fields.push(field({ type: '', name: 'comments', accessibleName: 'Comments' }));
  const f = detectAutocompleteIssues(s);
  assert(f.length === 0, 'comments textarea is clean', JSON.stringify(f));
}

// 14. Examples capped at 5 even with 8 missing-pii fields.
{
  const s = baseSnap();
  for (let i = 0; i < 8; i++) {
    s.fields.push(field({ name: `address${i}`, accessibleName: 'Address' }));
  }
  const f = detectAutocompleteIssues(s);
  const pii = f.find((x) => x.kind === 'autocomplete.missing-pii');
  const examples = pii?.evidence.examples as string[];
  assert(examples.length === 5, 'examples capped at 5', JSON.stringify(examples));
}

console.log('\n=== autocomplete.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
