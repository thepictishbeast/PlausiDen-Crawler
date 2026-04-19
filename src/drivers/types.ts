/**
 * Driver interface — every platform (web / android / ios / desktop)
 * implements this so the Runner can dispatch steps polymorphically.
 *
 * Rule of thumb: keep the method set MINIMAL. Any platform-specific
 * feature (Playwright trace, Maestro flows, Appium context switch)
 * goes in the driver's own module, not this interface.
 */
import type { CapturedEvent } from '../report';

export type Platform =
  | 'web'
  | 'android'
  | 'ios'
  | 'desktop-macos'
  | 'desktop-windows'
  | 'desktop-linux';

export interface Driver {
  readonly platform: Platform;
  /** Start the target session (open browser / launch app). */
  start(target: string): Promise<void>;
  goto(url: string): Promise<void>;
  click(selector: string, opts?: { timeout?: number }): Promise<void>;
  fill(selector: string, value: string, opts?: { timeout?: number }): Promise<void>;
  type(selector: string, value: string): Promise<void>;
  press(key: string, selector?: string): Promise<void>;
  waitFor(selector: string, opts?: { timeout?: number }): Promise<void>;
  waitMs(ms: number): Promise<void>;
  scroll(dy: number): Promise<void>;
  screenshot(path: string): Promise<void>;
  textOf(selector: string, opts?: { timeout?: number }): Promise<string | null>;
  /** Register a callback that fires for every event the driver captures
   *  (console, network, accessibility violation, etc). */
  onEvent(cb: (e: CapturedEvent) => void): void;
  /** Clean shutdown — close browser / end Appium session / kill adb tail. */
  close(): Promise<void>;
}
