/**
 * lighthouseAudit.ts — run Lighthouse as a LIBRARY and hand the result to
 * analytics.plausiden.com.
 *
 *   npm run audit:lighthouse -- --url https://plausiden.com/
 *   npm run audit:lighthouse -- --targets journeys/lighthouse-targets.json
 *   npm run audit:lighthouse -- --url https://x/ --no-write     (dry run)
 *
 * ## Why the Node API and not the CLI
 *
 * `lighthouse` the command and `@lhci/cli` both wrap the same library in
 * an orchestration layer — target lists, retries, output directories,
 * exit codes — every part of which this repo already has in
 * `crawler-runner` and `runs/`. Taking the CLI would mean maintaining two
 * runners that disagree about what a run is. The library is the part
 * that adds capability; the wrapper is the part that duplicates.
 *
 * ## Chrome
 *
 * Launched through `chrome-launcher` (already in Lighthouse's own tree)
 * pointed at the Chromium Playwright has already downloaded, so this adds
 * no second browser stack and no second multi-hundred-megabyte download.
 * Targets run SERIALLY. Parallel Chrome instances are how this host gets
 * wedged — it has previously accumulated 449 orphaned chromium-shell
 * processes at ~12.8GB.
 *
 * ## Exit codes, because a run that audited nothing must not look like
 * success
 *
 *   0  every target produced a usable run
 *   1  at least one target failed, was unusable, or could not be written
 *   2  the invocation itself was wrong: no target, unreadable or empty
 *      targets file
 *
 * A missing targets file exiting 0 would let the loop iterate nothing,
 * write nothing, and let systemd record a clean run forever.
 */
import lighthouse from 'lighthouse';
import { launch, type LaunchedChrome } from 'chrome-launcher';
import { chromium } from 'playwright';
import {
  mkdirSync,
  writeFileSync,
  renameSync,
  chmodSync,
  readFileSync,
  existsSync,
} from 'node:fs';
import { join } from 'node:path';
import { adapt, type RunState } from './lighthouseAdapter.js';

const DEFAULT_OUT = '/var/lib/plausiden-analytics/ux-inbox';
const DEFAULT_ARCHIVE = '/tank/scratch/uxaudit';
/** One target may not hang the whole unit until systemd kills it. */
const PER_TARGET_TIMEOUT_MS = 240_000;

type FormFactor = 'mobile' | 'desktop';
interface Target {
  url: string;
  form_factor: FormFactor;
}

/**
 * Pinned, and stored with every run.
 *
 * A Lighthouse score is not comparable across runs unless the throttling
 * and the emulated screen are the same, and the default simulated-4G
 * profile against a self-hosted origin produces alarming numbers that
 * mean nothing. `provided` means "measure what actually happened" — the
 * honest choice for an origin on the same network — and it is written
 * into the record so a future reader knows which world the number came
 * from rather than assuming.
 */
const THROTTLING = {
  method: 'provided',
  rtt_ms: 0,
  throughput_kbps: 0,
  cpu_slowdown: 1,
} as const;

const EMULATION: Record<FormFactor, Record<string, unknown>> = {
  mobile: { width: 412, height: 823, dpr: 1.75, mobile: true },
  desktop: { width: 1350, height: 940, dpr: 1, mobile: false },
};

const lhConfig = (ff: FormFactor) => ({
  logLevel: 'error' as const,
  output: 'json' as const,
  formFactor: ff,
  throttlingMethod: 'provided' as const,
  screenEmulation: {
    mobile: ff === 'mobile',
    width: EMULATION[ff].width as number,
    height: EMULATION[ff].height as number,
    deviceScaleFactor: EMULATION[ff].dpr as number,
    disabled: false,
  },
});

const arg = (name: string): string | undefined => {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 ? process.argv[i + 1] : undefined;
};
const flag = (name: string): boolean => process.argv.includes(`--${name}`);

const die = (code: number, msg: string): never => {
  console.error(`lighthouseAudit: ${msg}`);
  process.exit(code);
};

function loadTargets(): Target[] {
  const url = arg('url');
  const file = arg('targets');
  const ff = (arg('form-factor') as FormFactor) || 'mobile';
  if (url && file) die(2, 'pass --url or --targets, not both');
  if (ff !== 'mobile' && ff !== 'desktop') die(2, `--form-factor must be mobile or desktop, got ${ff}`);
  if (url) return [{ url, form_factor: ff }];
  if (!file) die(2, 'need --url <url> or --targets <file.json>');
  if (!existsSync(file!)) die(2, `targets file ${file} does not exist`);
  let parsed: unknown;
  try {
    parsed = JSON.parse(readFileSync(file!, 'utf8'));
  } catch (e) {
    return die(2, `targets file ${file} is not valid JSON: ${(e as Error).message}`);
  }
  const list = Array.isArray(parsed) ? parsed : (parsed as any)?.targets;
  if (!Array.isArray(list) || list.length === 0) {
    return die(2, `targets file ${file} lists no targets — a run over an empty list writes nothing and would otherwise exit 0`);
  }
  return list.map((t: any, i: number) => {
    if (!t || typeof t.url !== 'string' || t.url === '') {
      die(2, `target ${i} has no url`);
    }
    const f = t.form_factor ?? 'mobile';
    if (f !== 'mobile' && f !== 'desktop') {
      die(2, `target ${i} (${t.url}) has form_factor ${JSON.stringify(f)}; expected mobile or desktop`);
    }
    return { url: t.url, form_factor: f as FormFactor };
  });
}

/**
 * `.tmp` -> chmod -> rename.
 *
 * Not `writeFileSync(path, data, { mode })`: the mode passed to `open(2)`
 * is filtered by the writer's umask, and on this host umask 077 turns
 * 0640 into 0600 — which collapses the inbox directory's POSIX ACL mask
 * to `---` and leaves the analytics service unable to read a file that
 * looks perfectly fine in `ls`. `chmod(2)` is not umask-filtered. This
 * exact failure is documented three ways in
 * `plausiden-analytics-logperm.service`.
 *
 * The rename is what publishes the file: `rename(2)` within a directory
 * is atomic, so the reader can never see half of one.
 */
function writeAtomic(dir: string, name: string, body: string): void {
  const finalPath = join(dir, name);
  const tmp = `${finalPath}.tmp`;
  writeFileSync(tmp, body);
  chmodSync(tmp, 0o640);
  renameSync(tmp, finalPath);
}

async function runOne(
  target: Target,
  chrome: LaunchedChrome,
): Promise<{ records: unknown[]; state: RunState; runId: string; summary: string; lhr: any }> {
  const started = Date.now();
  const nowSec = Math.floor(started / 1000);
  let lhr: any = null;
  let error = '';
  try {
    const timeout = new Promise<never>((_, rej) =>
      setTimeout(
        () => rej(new Error(`timed out after ${PER_TARGET_TIMEOUT_MS}ms`)),
        PER_TARGET_TIMEOUT_MS,
      ),
    );
    const res: any = await Promise.race([
      lighthouse(target.url, { ...lhConfig(target.form_factor), port: chrome.port }),
      timeout,
    ]);
    lhr = res?.lhr ?? null;
    if (!lhr) error = 'Lighthouse resolved without an LHR';
  } catch (e) {
    error = (e as Error).message || String(e);
  }

  const out = adapt({
    requestedUrl: target.url,
    formFactor: target.form_factor,
    lhr,
    error,
    durationMs: Date.now() - started,
    nowSec,
    throttling: { ...THROTTLING },
    emulation: { ...EMULATION[target.form_factor] },
  });
  return { ...out, lhr };
}

async function main(): Promise<void> {
  const targets = loadTargets();
  const out = arg('out') ?? DEFAULT_OUT;
  const archive = arg('archive') ?? DEFAULT_ARCHIVE;
  const write = !flag('no-write');

  if (write) {
    try {
      mkdirSync(out, { recursive: true });
    } catch (e) {
      die(2, `output directory ${out} is not usable: ${(e as Error).message}`);
    }
  }

  let chrome: LaunchedChrome | null = null;
  let launchError = '';
  try {
    chrome = await launch({
      chromePath: chromium.executablePath(),
      chromeFlags: [
        '--headless=new',
        '--no-sandbox',
        '--disable-gpu',
        '--disable-dev-shm-usage',
      ],
    });
  } catch (e) {
    launchError = `could not launch Chromium: ${(e as Error).message}`;
  }

  let wrote = 0;
  let bad = 0;

  try {
    for (const t of targets) {
      const res = chrome
        ? await runOne(t, chrome)
        : {
            ...adapt({
              requestedUrl: t.url,
              formFactor: t.form_factor,
              lhr: null,
              error: launchError,
              durationMs: 0,
              nowSec: Math.floor(Date.now() / 1000),
              throttling: { ...THROTTLING },
              emulation: { ...EMULATION[t.form_factor] },
            }),
            lhr: null,
          };

      console.log(res.summary);
      if (res.state !== 'clean' && res.state !== 'findings') bad += 1;

      // The raw LHR goes to scratch, never the inbox: it is forensics,
      // not a record, it is ~2MB of nested detail, and the inbox is
      // credential-scanned on the assumption it holds small flat rows.
      if (write && res.lhr) {
        try {
          const dir = join(archive, res.runId.replace(/[^A-Za-z0-9._|-]/g, '_'));
          mkdirSync(dir, { recursive: true });
          writeFileSync(join(dir, 'lhr.json'), JSON.stringify(res.lhr));
        } catch (e) {
          console.error(`could not archive raw LHR: ${(e as Error).message}`);
        }
      }

      if (!write) continue;
      const body = res.records.map((r) => JSON.stringify(r)).join('\n') + '\n';
      const name = `${res.runId.replace(/[^A-Za-z0-9._-]/g, '_')}.jsonl`;
      try {
        writeAtomic(out, name, body);
        wrote += 1;
        console.log(`  wrote ${join(out, name)} (${res.records.length} records)`);
      } catch (e) {
        bad += 1;
        console.error(`  COULD NOT WRITE ${join(out, name)}: ${(e as Error).message}`);
      }
    }
  } finally {
    if (chrome) await chrome.kill();
  }

  if (!write) {
    console.log(`dry run: ${targets.length} target(s) audited, nothing written`);
    process.exit(bad > 0 ? 1 : 0);
  }
  if (wrote === 0) {
    die(1, `audited ${targets.length} target(s) and wrote nothing — this is a failure, not a clean run`);
  }
  console.log(`${wrote}/${targets.length} run(s) written to ${out}; ${bad} not usable`);
  process.exit(bad > 0 ? 1 : 0);
}

main().catch((e) => {
  // Nothing below main is allowed to swallow this. An unhandled
  // rejection that exits 0 is the silent pass this whole file exists to
  // prevent.
  console.error(`lighthouseAudit: unhandled failure: ${(e as Error).stack || e}`);
  process.exit(1);
});
