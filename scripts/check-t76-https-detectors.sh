#!/usr/bin/env bash
# check-t76-https-detectors.sh — automated liveness check for the
# T76 response-header + mixed-content detectors that require an
# HTTPS-served fixture page.
#
# Why this exists
#   The http fixture (fixtures/t76-detectors/) can't exercise
#   detectors that short-circuit on http or localhost — namely
#   hsts, xFrameOptions, and mixedContent (which only fires on
#   https pages). This script:
#     1. Spins up fixtures/t76-detectors-https/serve.py on port
#        8773 with a self-signed cert.
#     2. Sets CRAWLER_IGNORE_HTTPS_ERRORS=1 so Playwright trusts
#        the cert.
#     3. Sets CRAWLER_DISABLE_LOCALHOST_EXEMPTION=1 so the
#        hsts + xFrameOptions detectors fire on 127.0.0.1.
#     4. Runs the audit against journeys/t76-detector-fixtures-https.json.
#     5. Asserts each route's expectedFindingsByLabel was observed.
#     6. Cleans up.
#
# Usage:
#   scripts/check-t76-https-detectors.sh
#   scripts/check-t76-https-detectors.sh --keep      # keep run dir
#
# Exit codes:
#   0 — all expected findings observed
#   1 — at least one expected finding missing
#   2 — infrastructure error (missing cert, server didn't start, etc.)

set -euo pipefail

PORT="${PORT:-8773}"
KEEP_RUN=0
for arg in "$@"; do
  case "$arg" in
    --keep) KEEP_RUN=1 ;;
    --help|-h)
      sed -n '/^# /p' "$0" | sed 's/^# //'
      exit 0
      ;;
    *)
      echo "unknown arg: $arg" >&2
      exit 2
      ;;
  esac
done

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
JOURNEY="$REPO_ROOT/journeys/t76-detector-fixtures-https.json"
FIXTURE_SERVE="$REPO_ROOT/fixtures/t76-detectors-https/serve.py"
CERT="$REPO_ROOT/fixtures/t76-detectors-https/cert.pem"
LOG_FILE="/tmp/t76-https-fixtures-${PORT}.log"

[ -f "$JOURNEY" ] || { echo "FATAL: $JOURNEY missing" >&2; exit 2; }
[ -f "$FIXTURE_SERVE" ] || { echo "FATAL: $FIXTURE_SERVE missing" >&2; exit 2; }
[ -f "$CERT" ] || { echo "FATAL: $CERT missing — run openssl req to regenerate" >&2; exit 2; }
command -v jq >/dev/null || { echo "FATAL: jq required" >&2; exit 2; }
command -v python3 >/dev/null || { echo "FATAL: python3 required" >&2; exit 2; }

echo "[check-t76-https] killing any stale fixture server on :$PORT"
EXISTING=$(ps -ef | grep -v grep | grep "serve.py --port $PORT" | awk '{print $2}' || true)
if [ -n "$EXISTING" ]; then
  # shellcheck disable=SC2086
  kill $EXISTING 2>/dev/null || true
  sleep 1
fi

echo "[check-t76-https] starting fixture server on :$PORT"
nohup python3 "$FIXTURE_SERVE" --port "$PORT" >"$LOG_FILE" 2>&1 &
SERVER_PID=$!

cleanup() {
  echo "[check-t76-https] cleanup: stopping fixture server pid=$SERVER_PID"
  kill "$SERVER_PID" 2>/dev/null || true
  if [ "$KEEP_RUN" = "0" ] && [ -n "${RUN_DIR:-}" ] && [ -d "$RUN_DIR" ]; then
    echo "[check-t76-https] cleanup: removing run dir $RUN_DIR"
    rm -rf "$RUN_DIR"
  fi
}
trap cleanup EXIT

# Wait up to 5s for the server. Use var-capture form rather than
# inline pipe; `set -e + 2>/dev/null | grep -q` kills the script
# between the curl and the grep (subtle bash issue with the
# combination — caught when the curl-then-grep loop silently exited
# before the loop's normal end).
SERVER_OK=0
for _ in $(seq 1 50); do
  # curl can exit non-zero AFTER printing %{http_code} when TLS
  # body-read flakes on the self-signed cert. Wrap with `|| true`
  # to swallow the exit code; we trust the printed status.
  code=$(curl -ks -o /dev/null -w '%{http_code}' "https://127.0.0.1:$PORT/control/" 2>/dev/null || true)
  if [ "$code" = "200" ]; then
    SERVER_OK=1
    break
  fi
  sleep 0.1
done
if [ "$SERVER_OK" != "1" ]; then
  echo "FATAL: fixture server didn't come up within 5s — see $LOG_FILE" >&2
  exit 2
fi

echo "[check-t76-https] running audit"
cd "$REPO_ROOT"
# Both env vars opt the audit into fixture-mode behaviour. Real
# audits NEVER set these.
CRAWLER_IGNORE_HTTPS_ERRORS=1 \
CRAWLER_DISABLE_LOCALHOST_EXEMPTION=1 \
  timeout 240 npm run audit -- --journey "$JOURNEY" >/tmp/t76-https-audit-stdout.log 2>&1 || true

RUN_DIR=$(ls -td "$REPO_ROOT/runs/t76-detector-fixtures-https-"* 2>/dev/null | head -1)
if [ -z "$RUN_DIR" ] || [ ! -f "$RUN_DIR/report.json" ]; then
  echo "FATAL: no report.json — see /tmp/t76-https-audit-stdout.log" >&2
  cat /tmp/t76-https-audit-stdout.log >&2
  exit 2
fi
echo "[check-t76-https] run dir: $RUN_DIR"

REPORT="$RUN_DIR/report.json"
if [ "$(jq -r '.events | type' "$REPORT")" != "array" ]; then
  echo "FATAL: report.events absent or wrong type" >&2
  exit 2
fi

EXPECTED_JSON=$(jq -r '.expectedFindingsByLabel' "$JOURNEY")
LABEL_URL_JSON=$(jq -c '
  .steps
  | map(select(.kind == "goto"))
  | map({key: .label, value: .url})
  | from_entries
' "$JOURNEY")

PASS=0
FAIL=0
MISSING=()
EXTRA_NOTE=()

LABELS=$(echo "$EXPECTED_JSON" | jq -r 'keys_unsorted[]')
while IFS= read -r LABEL; do
  EXPECTED_KINDS=$(echo "$EXPECTED_JSON" | jq -r --arg l "$LABEL" '.[$l][]?' 2>/dev/null)
  STEP_URL=$(echo "$LABEL_URL_JSON" | jq -r --arg l "$LABEL" '.[$l] // ""')
  [ -z "$STEP_URL" ] && continue

  OBSERVED=$(jq -r --arg u "$STEP_URL" '
    .events[]
    | select(.url == $u)
    | select(.ruleId != null and .ruleId != "")
    | .ruleId
  ' "$REPORT" | sort -u)

  if [ -z "$EXPECTED_KINDS" ]; then
    if [ -n "$OBSERVED" ]; then
      EXTRA_NOTE+=("control route '$LABEL' produced findings: $(echo "$OBSERVED" | tr '\n' ' ')")
    fi
    PASS=$((PASS + 1))
    continue
  fi

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

echo
echo "==== T76 HTTPS detector liveness check ===="
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
  echo "RESULT: FAIL"
  exit 1
fi
echo
echo "RESULT: PASS — every expected HTTPS detector finding observed."
