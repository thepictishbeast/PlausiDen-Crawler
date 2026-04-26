#!/usr/bin/env bash
# capture-login.sh — one-time interactive Playwright login.
#
# Why this exists:
#   The Sacred.Vote authenticated journeys (admin / voter) need to crawl
#   pages behind a login. We deliberately do NOT insert fake admin or
#   voter rows into the production database (hard rule: prod is real
#   users only). Instead, the operator logs in once with a real
#   credential, and Playwright captures the resulting cookies +
#   localStorage as a `storageState` JSON file in ~/.secure/. The
#   crawler then re-loads that state on every run so the sweep is
#   authenticated without any DB writes.
#
# Usage:
#   scripts/capture-login.sh admin
#   scripts/capture-login.sh voter
#
# What happens:
#   1. Chromium launches in HEADFUL mode (visible window).
#   2. The crawler navigates to https://sacred.vote/<role>
#   3. You log in by hand (admin code + 2FA, or voter code).
#   4. The crawler waits 90s for the login to complete then dumps
#      cookies + localStorage to ~/.secure/sacred-vote-<role>-state.json
#   5. The file is mode 600 so only `admin` can read it.
#
# Re-run whenever the session expires (cookies / refresh tokens roll
# over). Sacred.Vote sessions appear to last days-to-weeks; expect to
# refresh once per fortnight at most.
set -euo pipefail

ROLE=${1:-}
case "$ROLE" in
  admin|voter) ;;
  *) echo "usage: $0 {admin|voter}" >&2; exit 2 ;;
esac

STATE="$HOME/.secure/sacred-vote-$ROLE-state.json"
URL="https://sacred.vote"
[ "$ROLE" = "admin" ] && URL="$URL/admin"

mkdir -p "$HOME/.secure"
chmod 700 "$HOME/.secure" 2>/dev/null || true

cd "$(dirname "$0")/.."

cat <<EOF
[capture] launching Chromium HEADFUL — log in to $ROLE in the browser
[capture] target URL: $URL
[capture] storageState target: $STATE
[capture] you have ~90s to complete the login before capture
EOF

# Synthesize a one-step journey: goto + long wait so the human can log in
# at their own pace before the state is captured at the end of the run.
TMP_JOURNEY=$(mktemp /tmp/capture-XXXXXX.json)
trap 'rm -f "$TMP_JOURNEY"' EXIT
cat > "$TMP_JOURNEY" <<JSON
{
  "name": "capture-$ROLE-login",
  "description": "One-time interactive login capture for $ROLE.",
  "baseUrl": "$URL",
  "steps": [
    { "kind": "goto", "url": "$URL", "timeout": 30000, "label": "login-page" },
    { "kind": "wait", "ms": 90000, "label": "manual-login-window" },
    { "kind": "screenshot", "label": "post-login" }
  ]
}
JSON

HEADFUL=1 node --loader ts-node/esm src/main.ts \
  --journey "$TMP_JOURNEY" \
  --save-state "$STATE"

chmod 600 "$STATE" 2>/dev/null || true
echo
echo "[capture] saved storageState to $STATE"
echo "[capture] now run:  node --loader ts-node/esm src/main.ts --journey journeys/sacred-vote-$ROLE.json"
