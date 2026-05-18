import type { Budget } from './types.js';

export const DEFAULT_BUDGET: Budget = {
  newConsoleErrors: 0,
  newPageErrors: 0,
  newFailedRequests: 0,
  newA11yViolations: 0,
  newCssHealthStrict: 0,
  newUiOverflowStrict: 0,
  newRuntimeContrastStrict: 0,
  newRuntimeImagesStrict: 0,
  newRuntimeFocusStrict: 0,
  newWebVitalsStrict: 0,
  newCspViolations: 0,
  newAriaDriftStrict: 0,
  newHeadingOrderStrict: 0,
  newRuntimeLandmarksStrict: 0,
  newLinkTextStrict: 0,
  newPlaceholderTextStrict: 0,
  newlyBrokenSteps: 0,
};
