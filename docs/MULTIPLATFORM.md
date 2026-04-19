# Multi-platform scope — web, desktop, Android, iOS

User directive 2026-04-19: the crawler must eventually drive **any website, any desktop app, any Android app, any iOS app** — all through the same journey + report layer.

## Architecture

Driver-agnostic runner. The only thing the `Runner` knows is the journey (step list) + the `Report`. Each step is dispatched to a platform-specific `Driver` that implements a narrow interface:

```ts
export interface Driver {
  platform: 'web' | 'android' | 'ios' | 'desktop-macos' | 'desktop-windows' | 'desktop-linux';
  start(target: string): Promise<void>;
  // Canonical step actions, mirroring journey.ts:
  goto(url: string): Promise<void>;
  click(selector: string, opts?: { timeout?: number }): Promise<void>;
  fill(selector: string, value: string): Promise<void>;
  press(key: string, selector?: string): Promise<void>;
  screenshot(path: string): Promise<void>;
  waitFor(selector: string, opts?: { timeout?: number }): Promise<void>;
  // Event capture — drivers push events into the Report's collector.
  onEvent(cb: (e: CapturedEvent) => void): void;
  close(): Promise<void>;
}
```

A single journey file runs across drivers; the selector syntax may differ per platform (CSS for web, UI-automator/XCUITest/accessibility-id for mobile/desktop) but that's per-step metadata.

## Driver implementations + FOSS candidates

### Web
- **Primary:** Playwright (Apache 2.0, Microsoft). Already in v0.1. Fastest, best event capture (CDP).
- **Fallback:** Selenium WebDriver (Apache 2.0) — only if running in an env Playwright can't install.

### Android
- **Primary: Maestro** (Apache 2.0, mobile.dev) — YAML-native flows, fast startup, cross-platform with iOS, captures logs + screenshots + video by default. Single Go binary, no JDK required.
- **Alternative: Appium + UIAutomator2** (Apache 2.0) — battle-tested, more setup overhead (needs Node + ADB).
- **Internal option:** Espresso/UIAutomator directly — only if we own an app we can instrument.

### iOS
- **Primary: Maestro** — same tool as Android; one binary drives both.
- **Alternative: Appium + XCUITest driver** (Apache 2.0) — requires a Mac host for real-device runs. Usable for simulator runs anywhere XCTest can build.
- **Direct: XCUITest** (Apple) — MIT-compatible (via Xcode license) but Mac-only.

### Desktop
- **macOS:** Appium Mac2 Driver (Apache 2.0) — XCUI under the hood.
- **Windows:** WinAppDriver (MIT, Microsoft) + Appium Windows Driver.
- **Linux:** Appium Linux driver OR `xdotool`+screenshot for older apps; for Tauri/Electron apps specifically, use Playwright's CDP path (these apps speak DevTools protocol).

### Log capture across platforms
- Web: `page.on('console')` — already have.
- Android: `adb logcat -b all` tailed during the run, filtered by app package.
- iOS: `xcrun simctl spawn <dev> log stream --predicate 'subsystem == "<bundle-id>"'` (simulator) or Instruments stream (device).
- Desktop: platform-specific — stdout/stderr of the launched process on macOS/Linux; ETW trace on Windows.

All funnel through the same `CapturedEvent` shape so the diff algorithm is platform-agnostic.

## FOSS projects to fork/vendor (updated for multi-platform)

Tier A — absorb:

| Project | License | Platforms | Fit |
|---|---|---|---|
| **Playwright** | Apache 2.0 | Web + Tauri/Electron via CDP | ✅ Web driver (already) |
| **Maestro** | Apache 2.0 | Android + iOS | ✅ Mobile driver — YAML flows align perfectly with our step JSON |
| **Appium** | Apache 2.0 | Everything else (Desktop, Win/macOS, legacy mobile) | ✅ Catch-all driver; use selectively |
| **axe-core** | MPL-2.0 | Web | ✅ Accessibility rule engine (web-only for now) |
| **Lighthouse CI** | Apache 2.0 | Web | ✅ Perf + CSP (web-only) |

Tier B — evaluate later:

| Project | License | Platforms | Notes |
|---|---|---|---|
| Detox | MIT | React Native only | If we end up testing RN apps specifically |
| WebDriverIO | MIT | Wrapper over Appium/Selenium/Playwright | Worth revisiting if our driver layer grows unwieldy |
| Carina | Apache 2.0 | Java cross-platform | Heavy JVM dep; skip unless a consumer requires Java |
| Cypress | MIT | Web only | Redundant with Playwright |

## Directory layout (target v0.3+)

```
PlausiDen-Crawler/
├── src/
│   ├── runner.ts              # dispatches steps to the right driver
│   ├── journey.ts             # step format (platform-agnostic)
│   ├── report.ts              # diff + serialize
│   ├── drivers/
│   │   ├── web-playwright.ts
│   │   ├── web-lighthouse.ts       # secondary pass, perf/CSP focus
│   │   ├── mobile-maestro.ts       # Android + iOS
│   │   ├── appium-generic.ts       # fallback for desktop / niche
│   │   └── types.ts                # Driver interface
│   └── adapters/
│       ├── axe.ts                  # web-only a11y rules
│       └── lighthouse.ts
├── journeys/
│   ├── plausiden-web.json
│   ├── plausiden-android.json      # will use Maestro-style selectors
│   └── plausiden-ios.json
├── docs/
│   ├── PLAN.md
│   ├── FOSS_FORK_CANDIDATES.md
│   └── MULTIPLATFORM.md            # this file
└── vendor/                         # forked FOSS (thin)
```

## Rollout plan

- **v0.2** (now) — web driver solid + axe-core adapter + journey JSON + diff.
- **v0.3** — add Maestro driver for Android. Journey format unchanged; steps for mobile use `accessibility-id` / `text` selectors.
- **v0.4** — iOS via Maestro. Basically free since it's the same binary.
- **v0.5** — Appium fallback for desktop apps (macOS first).
- **v0.6** — Windows + Linux desktop drivers.
- **v0.7** — unified CLI: `crawler --platform web --journey smoke.json --target https://...` or `--platform android --journey mobile.json --device emulator-5554`.
