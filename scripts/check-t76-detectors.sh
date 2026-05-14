#!/usr/bin/env bash
# check-t76-detectors.sh — automated liveness check for the T76
# detector axes.
#
# Why this exists:
#   The 2026-05-14 NaN-evalFn bug silently broke the formLabels
#   detector across every page of every audit. No existing test
#   caught it because every test exercised the detector in
#   isolation — not through the live page.evaluate pipeline. The
#   regression-guard fixture set (fixtures/t76-detectors/) gives us
#   30 routes that each SHOULD trigger one specific finding kind;
#   this script wires the fixture into an automated assertion so
#   future detector regressions get caught the next time this
#   runs, not the next time someone manually re-audits a real site.
#
# What it does:
#   1. Starts the fixture server on port 8771 in the background
#      (kills any stale instance first).
#   2. Runs `npm run audit` against journeys/t76-detector-fixtures.json.
#   3. Parses the produced report.json.
#   4. For each route in expectedFindingsByLabel: asserts that the
#      report.events stream contains an event whose `ruleId` matches
#      one of the expected finding kinds at that step.
#   5. Cleans up the fixture server and the run directory.
#   6. Exits 0 on PASS, non-zero on any missing expected finding.
#
# Usage:
#   scripts/check-t76-detectors.sh              # default port 8771
#   scripts/check-t76-detectors.sh --keep       # keep run-dir for inspection
#
# Exit codes:
#   0 — all expected findings observed
#   1 — at least one expected finding missing (detector likely
#       broken or mis-calibrated)
#   2 — infrastructure error (server failed to start, audit
#       crashed, jq not installed, etc.)

set -euo pipefail

PORT="${PORT:-8771}"
KEEP_RUN=0
for arg in "$@"; do
  case "$arg" in
    --keep) KEEP_RUN=1 ;;
    --help|-h)
      sed -n '/^# /p' "$0" | sed 's/^# //'
      exit 0
      ;;
    *)
      echo "unknown arg: $arg (use --help)" >&2
      exit 2
      ;;
  esac
done

# Resolve repo root from this script's location.
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
JOURNEY="$REPO_ROOT/journeys/t76-detector-fixtures.json"
FIXTURE_SERVE="$REPO_ROOT/fixtures/t76-detectors/serve.py"
LOG_FILE="/tmp/t76-fixtures-${PORT}.log"

if [ ! -f "$JOURNEY" ]; then
  echo "FATAL: journey not found: $JOURNEY" >&2
  exit 2
fi
if [ ! -f "$FIXTURE_SERVE" ]; then
  echo "FATAL: fixture server not found: $FIXTURE_SERVE" >&2
  exit 2
fi
if ! command -v jq >/dev/null 2>&1; then
  echo "FATAL: 'jq' not installed; required to parse report.json" >&2
  exit 2
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "FATAL: 'python3' not installed; required to run fixture server" >&2
  exit 2
fi

# ----- start fixture server -----
echo "[check-t76] killing any stale fixture server on :$PORT"
# Be specific so we don't kill unrelated python servers on other ports.
EXISTING_PIDS=$(ps -ef | grep -v grep | grep "serve.py --port $PORT" | awk '{print $2}' || true)
if [ -n "$EXISTING_PIDS" ]; then
  # shellcheck disable=SC2086
  kill $EXISTING_PIDS 2>/dev/null || true
  sleep 1
fi

echo "[check-t76] starting fixture server on :$PORT"
nohup python3 "$FIXTURE_SERVE" --port "$PORT" >"$LOG_FILE" 2>&1 &
SERVER_PID=$!

cleanup() {
  echo "[check-t76] cleanup: stopping fixture server pid=$SERVER_PID"
  kill "$SERVER_PID" 2>/dev/null || true
  if [ "$KEEP_RUN" = "0" ] && [ -n "${RUN_DIR:-}" ] && [ -d "$RUN_DIR" ]; then
    echo "[check-t76] cleanup: removing run dir $RUN_DIR"
    rm -rf "$RUN_DIR"
  fi
}
trap cleanup EXIT

# Wait for the server to come up (max 5s).
for _ in $(seq 1 50); do
  if curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/control/" 2>/dev/null | grep -q '^200$'; then
    break
  fi
  sleep 0.1
done
if ! curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/control/" 2>/dev/null | grep -q '^200$'; then
  echo "FATAL: fixture server didn't come up on :$PORT within 5s; see $LOG_FILE" >&2
  exit 2
fi

# ----- run the audit -----
echo "[check-t76] running audit"
cd "$REPO_ROOT"
# Cap audit at 4 minutes; the 30-step journey runs in ~30s normally.
if ! timeout 240 npm run audit -- --journey "$JOURNEY" >/tmp/t76-audit-stdout.log 2>&1; then
  EXIT=$?
  # Audit returns non-zero on findings (which is fine for our
  # purpose; we DELIBERATELY trigger findings). Only treat exit
  # codes that indicate infra failure (>=2 in our convention,
  # but the audit uses 1 for "regressions exceed budget" too).
  # Distinguish by checking whether report.json actually wrote.
  echo "[check-t76] audit exited $EXIT (expected — fixture deliberately triggers findings)"
fi

# ----- locate the latest run dir -----
RUN_DIR=$(ls -td "$REPO_ROOT/runs/t76-detector-fixtures-"* 2>/dev/null | head -1)
if [ -z "$RUN_DIR" ] || [ ! -f "$RUN_DIR/report.json" ]; then
  echo "FATAL: no report.json produced; audit infrastructure broken" >&2
  cat /tmp/t76-audit-stdout.log >&2
  exit 2
fi
echo "[check-t76] run dir: $RUN_DIR"

# ----- assert observed-vs-expected per label -----
REPORT="$RUN_DIR/report.json"

# Read the expected map from the journey.
EXPECTED_JSON=$(jq -r '.expectedFindingsByLabel' "$JOURNEY")
if [ "$EXPECTED_JSON" = "null" ]; then
  echo "FATAL: journey has no .expectedFindingsByLabel" >&2
  exit 2
fi

# We match observed findings to expected by URL, not by
# eventsByStep grouping. The latter's time-window slicing is
# coarse (windows are sized to step durationMs which often
# undershoots the actual goto+settle time, causing findings to
# spill into the NEXT step's window — an event grouped under
# `lang-empty` may actually have come from the next page's
# lang-unknown detector firing).
#
# The `events` stream carries each event's `url` field directly,
# so per-URL grouping is precise: a viewport.missing event whose
# url ends in `/no-viewport/` definitively came from the
# no-viewport page.
HAS_EVENTS=$(jq -r '.events | type' "$REPORT")
if [ "$HAS_EVENTS" != "array" ]; then
  echo "FATAL: report.events absent or wrong type ($HAS_EVENTS); audit shape changed?" >&2
  exit 2
fi

# Walk expected labels.
PASS=0
FAIL=0
MISSING=()
EXTRA_NOTE=()

LABELS=$(echo "$EXPECTED_JSON" | jq -r 'keys_unsorted[]')
# Resolve label → URL by reading the journey's steps array. Then
# the URL becomes the key for grouping report events.
LABEL_URL_JSON=$(jq -c '
  .steps
  | map(select(.kind == "goto"))
  | map({key: .label, value: .url})
  | from_entries
' "$JOURNEY")

while IFS= read -r LABEL; do
  EXPECTED_KINDS=$(echo "$EXPECTED_JSON" | jq -r --arg l "$LABEL" '.[$l][]?' 2>/dev/null)
  STEP_URL=$(echo "$LABEL_URL_JSON" | jq -r --arg l "$LABEL" '.[$l] // ""')

  if [ -z "$STEP_URL" ]; then
    echo "WARN: label '$LABEL' has no matching goto step in journey" >&2
    continue
  fi

  # Filter the report events to those whose `url` matches the
  # step URL and that carry a ruleId (= detector findings).
  # Each detector tags its event with the page URL it saw at
  # capture time, so this is a precise per-page grouping.
  OBSERVED=$(jq -r --arg u "$STEP_URL" '
    .events[]
    | select(.url == $u)
    | select(.ruleId != null and .ruleId != "")
    | .ruleId
  ' "$REPORT" | sort -u)

  if [ -z "$EXPECTED_KINDS" ]; then
    # No expected findings on this label — it's a control. Just
    # report briefly and move on.
    if [ -n "$OBSERVED" ]; then
      EXTRA_NOTE+=("control route '$LABEL' produced findings: $(echo "$OBSERVED" | tr '\n' ' ')")
    fi
    PASS=$((PASS + 1))
    continue
  fi

  # For each expected finding kind, assert it's in OBSERVED.
  LABEL_OK=1
  while IFS= read -r KIND; do
    if echo "$OBSERVED" | grep -Fxq "$KIND"; then
      :
    else
      MISSING+=("$LABEL: expected '$KIND' not observed")
      LABEL_OK=0
    fi
  done <<< "$EXPECTED_KINDS"
  if [ "$LABEL_OK" = "1" ]; then
    PASS=$((PASS + 1))
  else
    FAIL=$((FAIL + 1))
  fi
done <<< "$LABELS"

# ----- report -----
echo
echo "==== T76 detector liveness check ===="
echo "  routes checked:  $((PASS + FAIL))"
echo "  routes PASS:     $PASS"
echo "  routes FAIL:     $FAIL"
if [ ${#MISSING[@]} -gt 0 ]; then
  echo
  echo "MISSING expected findings:"
  for m in "${MISSING[@]}"; do
    echo "  ✗ $m"
  done
fi
if [ ${#EXTRA_NOTE[@]} -gt 0 ]; then
  echo
  echo "Notes (non-fatal):"
  for n in "${EXTRA_NOTE[@]}"; do
    echo "  · $n"
  done
fi

if [ "$FAIL" -gt 0 ]; then
  echo
  echo "RESULT: FAIL — at least one detector axis didn't fire its expected finding."
  echo "Likely causes: detector parser regression, JS string truncation, snapshot-shape drift."
  echo "Run with --keep to retain $RUN_DIR for inspection."
  exit 1
fi
echo
echo "RESULT: PASS — every expected finding observed across all routes."
