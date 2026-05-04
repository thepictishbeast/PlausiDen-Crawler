/**
 * cssHealthEndToEnd.ts — drive Playwright over the fixture server
 * and verify each fixture path produces the EXPECTED finding kind.
 *
 * Usage:
 *   npx tsx src/cssHealthEndToEnd.ts
 *
 * Assumes the fixture server is already running on
 * http://127.0.0.1:8765 (start with: python3
 * fixtures/css-health/serve.py).
 *
 * Exit code 0 if every fixture matches its expectation, 1 otherwise.
 *
 * This is the "function like a real visitor" proof — only the browser
 * touches the test pages; no file-system reads of the source.
 */
import { chromium, type BrowserContext, type Page } from 'playwright';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { captureCSSHealthSnapshot, detectCSSHealthIssues } from './cssHealth.js';

const __dirname = dirname(fileURLToPath(import.meta.url));
const journeyPath = join(__dirname, '..', 'journeys', 'css-health-fixtures.json');

type Journey = {
  baseUrl: string;
  steps: { kind: string; url: string; screenshot?: string }[];
  expectedFindingsByPath: Record<string, string[]>;
};

async function main(): Promise<void> {
  const journey: Journey = JSON.parse(readFileSync(journeyPath, 'utf-8'));

  const browser = await chromium.launch();
  const ctx: BrowserContext = await browser.newContext();
  const page: Page = await ctx.newPage();

  const networkResponses = new Map<
    string,
    { status: number; contentType: string | null; bodyBytes: number; errorText: string | null }
  >();

  page.on('response', async (resp) => {
    try {
      const url = resp.url();
      const headers = resp.headers();
      let bytes = 0;
      try {
        const buf = await resp.body();
        bytes = buf.length;
      } catch {
        bytes = 0;
      }
      networkResponses.set(url, {
        status: resp.status(),
        contentType: headers['content-type'] ?? null,
        bodyBytes: bytes,
        errorText: null,
      });
    } catch (e) {
      // Best-effort capture; ignore.
    }
  });

  page.on('requestfailed', (req) => {
    networkResponses.set(req.url(), {
      status: 0,
      contentType: null,
      bodyBytes: 0,
      errorText: req.failure()?.errorText ?? 'request failed',
    });
  });

  const failures: string[] = [];
  const passes: string[] = [];

  for (const step of journey.steps) {
    const url = step.url;
    networkResponses.clear();

    let nav: Awaited<ReturnType<Page['goto']>> = null;
    try {
      nav = await page.goto(url, { waitUntil: 'networkidle', timeout: 8000 });
    } catch (e) {
      failures.push(`${url}: navigation threw ${(e as Error).message}`);
      continue;
    }

    // Wait briefly for any sheet to apply.
    await page.waitForTimeout(300);

    const { snapshot, findings } = {
      snapshot: await captureCSSHealthSnapshot(page, networkResponses),
      findings: detectCSSHealthIssues(
        await captureCSSHealthSnapshot(page, networkResponses),
      ),
    };

    const path = new URL(url).pathname;
    const expected = journey.expectedFindingsByPath[path] ?? [];
    const gotKinds = findings.map((f) => f.kind);

    if (expected.length === 0) {
      if (findings.length === 0) {
        passes.push(`${path}: no findings (as expected)`);
      } else {
        failures.push(
          `${path}: expected NO findings but got ${JSON.stringify(gotKinds)} — snapshot ${JSON.stringify(snapshot.computedBody)}`,
        );
      }
    } else {
      const matched = expected.some((e) => gotKinds.includes(e));
      if (matched) {
        passes.push(`${path}: matched ${JSON.stringify(gotKinds)} (expected one of ${JSON.stringify(expected)})`);
      } else {
        failures.push(
          `${path}: expected one of ${JSON.stringify(expected)} but got ${JSON.stringify(gotKinds)} — snapshot ${JSON.stringify({ sheets: snapshot.declaredSheets, body: snapshot.computedBody, applied: snapshot.appliedRuleCountEstimate })}`,
        );
      }
    }
  }

  await browser.close();

  console.log(`\n=== cssHealthEndToEnd ===`);
  console.log(`PASSED ${passes.length}:`);
  passes.forEach((p) => console.log(`  ✓ ${p}`));
  if (failures.length > 0) {
    console.log(`FAILED ${failures.length}:`);
    failures.forEach((f) => console.log(`  ✗ ${f}`));
    process.exit(1);
  }
  console.log(`All ${passes.length} fixture(s) matched expectations.`);
}

main().catch((e) => {
  console.error(e);
  process.exit(2);
});
