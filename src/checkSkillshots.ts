import { chromium } from 'playwright';
import { captureCSSHealthSnapshot, detectCSSHealthIssues } from './cssHealth.js';

async function main() {
  const browser = await chromium.launch();
  const ctx = await browser.newContext();
  const page = await ctx.newPage();

  const networkResponses = new Map<string, { status: number; contentType: string | null; bodyBytes: number; errorText: string | null }>();
  page.on('response', async (resp) => {
    try {
      const url = resp.url();
      const headers = resp.headers();
      let bytes = 0; try { const buf = await resp.body(); bytes = buf.length; } catch {}
      networkResponses.set(url, { status: resp.status(), contentType: headers['content-type'] ?? null, bodyBytes: bytes, errorText: null });
    } catch {}
  });
  page.on('requestfailed', (req) => {
    networkResponses.set(req.url(), { status: 0, contentType: null, bodyBytes: 0, errorText: req.failure()?.errorText ?? 'failed' });
  });

  await page.goto('http://127.0.0.1:8123/', { waitUntil: 'networkidle', timeout: 8000 });
  await page.waitForTimeout(400);
  const snap = await captureCSSHealthSnapshot(page, networkResponses);
  const findings = detectCSSHealthIssues(snap);

  console.log('--snapshot--');
  console.log(JSON.stringify({
    pageUrl: snap.pageUrl,
    declaredSheets: snap.declaredSheets.map(s => ({ url: s.url.replace(/^http:\/\/127\.0\.0\.1:\d+/, ''), status: s.status, contentType: s.contentType, bodyBytes: s.bodyBytes, openBraces: s.declaredBraceCount, closeBraces: s.declaredCloseBraceCount })),
    computedBody: snap.computedBody,
    appliedRuleCountEstimate: snap.appliedRuleCountEstimate,
  }, null, 2));
  console.log('--findings--');
  console.log(JSON.stringify(findings, null, 2));
  console.log('--summary--');
  console.log(`${findings.length} finding(s); ${findings.filter(f => f.severity === 'strict').length} strict, ${findings.filter(f => f.severity === 'warn').length} warn`);

  await browser.close();
  process.exit(findings.filter(f => f.severity === 'strict').length > 0 ? 1 : 0);
}
main().catch(e => { console.error(e); process.exit(2); });
