/**
 * permissionsPolicy.test.ts — pure-function tests for the
 * Permissions-Policy detector (T76).
 */
import {
  buildPermissionsPolicySnapshot,
  detectPermissionsPolicyIssues,
} from './permissionsPolicy.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

// 1. No header → missing finding (warn).
{
  const s = buildPermissionsPolicySnapshot('https://example.com/', {});
  const f = detectPermissionsPolicyIssues(s);
  assert(
    f.length === 1 && f[0].kind === 'permissions-policy.missing' && f[0].severity === 'warn',
    'no header → missing warn',
    JSON.stringify(f),
  );
}

// 2. localhost exempt.
{
  const s = buildPermissionsPolicySnapshot('https://localhost:3000/', {});
  const f = detectPermissionsPolicyIssues(s);
  assert(f.length === 0, 'localhost exempt', JSON.stringify(f));
}

// 3. Comprehensive deny policy → no findings.
{
  const denyAll = [
    'camera', 'microphone', 'geolocation', 'payment', 'usb', 'serial', 'midi',
    'hid', 'bluetooth', 'accelerometer', 'gyroscope', 'magnetometer',
    'display-capture', 'screen-wake-lock',
  ].map((f) => `${f}=()`).join(', ');
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'permissions-policy': denyAll,
  });
  const f = detectPermissionsPolicyIssues(s);
  assert(f.length === 0, 'comprehensive deny → no findings', JSON.stringify(f));
}

// 4. Header value all garbage → invalid.
{
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'permissions-policy': 'totally not a policy at all',
  });
  const f = detectPermissionsPolicyIssues(s);
  assert(
    f.some((x) => x.kind === 'permissions-policy.invalid'),
    'pure garbage → invalid warn',
    JSON.stringify(f),
  );
}

// 5. camera=* with other directives → strict allow-all-camera.
{
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'permissions-policy': 'camera=*, microphone=(), geolocation=(), payment=(), usb=(), serial=(), midi=(), hid=(), bluetooth=(), accelerometer=(), gyroscope=(), magnetometer=(), display-capture=(), screen-wake-lock=()',
  });
  const f = detectPermissionsPolicyIssues(s);
  assert(
    f.some((x) => x.kind === 'permissions-policy.allow-all-camera' && x.severity === 'strict'),
    'camera=* → strict allow-all-camera',
    JSON.stringify(f),
  );
}

// 6. Multiple high-risk allow-alls → multiple strict findings.
{
  const allOpen = [
    'camera', 'microphone', 'geolocation', 'payment', 'usb', 'serial', 'midi',
    'hid', 'bluetooth', 'accelerometer', 'gyroscope', 'magnetometer',
    'display-capture', 'screen-wake-lock',
  ].map((f) => `${f}=*`).join(', ');
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'permissions-policy': allOpen,
  });
  const f = detectPermissionsPolicyIssues(s);
  const strictCount = f.filter((x) => x.severity === 'strict').length;
  assert(strictCount === 14, 'all open → 14 strict findings', `got ${strictCount}: ${JSON.stringify(f.map((x) => x.kind))}`);
}

// 7. Partial policy omits high-risk → high-risk-omitted warn.
{
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'permissions-policy': 'autoplay=(), fullscreen=(self)',
  });
  const f = detectPermissionsPolicyIssues(s);
  assert(
    f.some((x) => x.kind === 'permissions-policy.high-risk-omitted'),
    'partial policy → omitted warn',
    JSON.stringify(f),
  );
}

// 8. self-only is acceptable.
{
  const denyAll = [
    'microphone', 'geolocation', 'payment', 'usb', 'serial', 'midi',
    'hid', 'bluetooth', 'accelerometer', 'gyroscope', 'magnetometer',
    'display-capture', 'screen-wake-lock',
  ].map((f) => `${f}=()`).join(', ');
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'permissions-policy': `camera=(self), ${denyAll}`,
  });
  const f = detectPermissionsPolicyIssues(s);
  assert(f.length === 0, 'camera=(self) acceptable', JSON.stringify(f));
}

// 9. Self-only with origin allowlist is acceptable.
{
  const denyAll = [
    'microphone', 'geolocation', 'usb', 'serial', 'midi',
    'hid', 'bluetooth', 'accelerometer', 'gyroscope', 'magnetometer',
    'display-capture', 'screen-wake-lock',
  ].map((f) => `${f}=()`).join(', ');
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'permissions-policy': `payment=(self "https://stripe.com"), camera=(self), ${denyAll}`,
  });
  const f = detectPermissionsPolicyIssues(s);
  assert(f.length === 0, 'origin allowlist acceptable', JSON.stringify(f));
}

// 10. Bare `feature=self` shorthand parsed as self-only.
{
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'permissions-policy': 'camera=self',
  });
  const f = detectPermissionsPolicyIssues(s);
  assert(
    !f.some((x) => x.kind === 'permissions-policy.allow-all-camera'),
    'feature=self shorthand parses as self-only',
    JSON.stringify(f),
  );
}

// 11. Unknown feature is ignored (not flagged).
{
  const denyAll = [
    'camera', 'microphone', 'geolocation', 'payment', 'usb', 'serial', 'midi',
    'hid', 'bluetooth', 'accelerometer', 'gyroscope', 'magnetometer',
    'display-capture', 'screen-wake-lock',
  ].map((f) => `${f}=()`).join(', ');
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'permissions-policy': `made-up-feature=*, ${denyAll}`,
  });
  const f = detectPermissionsPolicyIssues(s);
  assert(f.length === 0, 'unknown feature ignored', JSON.stringify(f));
}

// 12. Header name case-insensitive.
{
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'Permissions-Policy': 'camera=()',
  });
  assert(s.raw === 'camera=()', 'header name case-insensitive', JSON.stringify(s));
}

// 13. Mixed allow-all + deny → only allow-all flagged.
{
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'permissions-policy': 'camera=(), microphone=*, geolocation=(self), payment=()',
  });
  const f = detectPermissionsPolicyIssues(s);
  const strictCount = f.filter((x) => x.severity === 'strict').length;
  assert(
    strictCount === 1 && f.some((x) => x.kind === 'permissions-policy.allow-all-microphone'),
    'mixed → only mic flagged strict',
    `strictCount=${strictCount} f=${JSON.stringify(f.map((x) => x.kind))}`,
  );
}

// 14. Empty header value → invalid (treat as garbage).
{
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'permissions-policy': '',
  });
  // Empty header means raw is "" but no directives parsed.
  assert(s.raw === '' && s.directives.length === 0, 'empty header → empty directives', JSON.stringify(s));
}

// 15. Trailing comma tolerated.
{
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'permissions-policy': 'camera=(),',
  });
  assert(s.directives.length === 1 && s.directives[0].isDeny, 'trailing comma tolerated', JSON.stringify(s));
}

// 16. Whitespace inside parens tolerated.
{
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'permissions-policy': 'camera=(   self    "https://e.com"   )',
  });
  const d = s.directives[0];
  assert(
    d.hasSelf && d.origins.length === 1 && d.origins[0] === 'https://e.com',
    'whitespace tolerated in allowlist',
    JSON.stringify(s),
  );
}

// 17. Single quotes in allowlist tolerated (non-spec but common).
{
  const s = buildPermissionsPolicySnapshot('https://example.com/', {
    'permissions-policy': "camera=('https://e.com')",
  });
  const d = s.directives[0];
  assert(
    d.origins.length === 1 && d.origins[0] === 'https://e.com',
    'single quotes tolerated',
    JSON.stringify(s),
  );
}

console.log('\n=== permissionsPolicy.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
