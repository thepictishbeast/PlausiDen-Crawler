#!/usr/bin/env bash
# seed-auth.sh — create a Sacred.Vote auth seed file the crawler can replay.
#
# Why this exists:
#   The Sacred.Vote admin SPA stores its session in sessionStorage (not
#   localStorage), which Playwright's storageState cannot capture. So the
#   classic "log in interactively, dump storageState" path doesn't work
#   for /admin. Instead we hit the API directly with the existing
#   ADMIN_CODE from .env, capture the CSRF token, and write a small JSON
#   the crawler injects via addInitScript on every page load.
#
#   For voter, /voting-app uses TEST mode (built into the server — no DB
#   writes). The seed file just records the TEST code so the crawler's
#   gatekeeper auto-filler knows what to type.
#
# This never inserts fake credentials into the database. It uses the
# existing prod admin code (admin reads /var/www/sacred.vote/.env) and
# the built-in TEST voter code. The resulting seed file lives at
# ~/.secure/sacred-vote-{role}-state.json, mode 600.
#
# Usage:
#   scripts/seed-auth.sh admin
#   scripts/seed-auth.sh voter
#
# Re-run before each crawl — the admin CSRF token expires after 24h
# absolute / 10m of inactivity. The script is idempotent.
set -euo pipefail

ROLE=${1:-}
case "$ROLE" in
  admin|voter) ;;
  *) echo "usage: $0 {admin|voter}" >&2; exit 2 ;;
esac

OUT="$HOME/.secure/sacred-vote-$ROLE-state.json"
BASE_URL=${SACRED_VOTE_URL:-https://sacred.vote}

mkdir -p "$HOME/.secure"
chmod 700 "$HOME/.secure" 2>/dev/null || true

if [ "$ROLE" = "admin" ]; then
  ENV_FILE=/var/www/sacred.vote/.env
  if [ ! -r "$ENV_FILE" ]; then
    echo "[seed] cannot read $ENV_FILE — need admin group membership" >&2
    exit 3
  fi
  ADMIN_CODE=$(grep -E '^ADMIN_CODE=' "$ENV_FILE" | head -n1 | sed -E "s/^ADMIN_CODE=['\"]?(.*)['\"]?$/\1/" | sed "s/'$//")
  if [ -z "$ADMIN_CODE" ]; then
    echo "[seed] ADMIN_CODE not found in $ENV_FILE" >&2
    exit 3
  fi

  RESP=$(curl -fsS -X POST "$BASE_URL/api/auth/verify-admin" \
    -H "Content-Type: application/json" \
    -H "User-Agent: PlausiDen-Crawler/seed-auth" \
    --data-raw "$(printf '{"code":"%s"}' "$ADMIN_CODE")") || {
    echo "[seed] verify-admin failed" >&2; exit 4;
  }

  CSRF=$(echo "$RESP" | python3 -c 'import sys, json; print(json.load(sys.stdin)["csrfToken"])')
  LABEL=$(echo "$RESP" | python3 -c 'import sys, json; print(json.load(sys.stdin).get("adminLabel", "admin_primary"))')

  python3 - "$OUT" "$ADMIN_CODE" "$CSRF" "$LABEL" <<'PY'
import json, os, sys
out, code, csrf, label = sys.argv[1:5]
seed = {
    "role": "admin",
    "sessionStorage": {
        "sv_admin_code": code,
        "sv_csrf_token": csrf,
        "sv_admin_label": label,
        "sv_admin_auth": "true",
    },
    "createdAt": __import__("datetime").datetime.utcnow().isoformat() + "Z",
    "ttlNote": "csrfToken: 24h absolute / 10m inactivity",
}
with open(out, "w") as f:
    json.dump(seed, f, indent=2)
os.chmod(out, 0o600)
PY

  echo "[seed] admin seed written to $OUT (label=$LABEL)"

else
  # Voter: TEST is a built-in code that the server recognizes without
  # any DB lookup. We pre-verify it to confirm the server is up and the
  # mode is enabled, then write a seed for the auto-gatekeeper filler.
  RESP=$(curl -fsS -X POST "$BASE_URL/api/auth/verify-voter" \
    -H "Content-Type: application/json" \
    -H "User-Agent: PlausiDen-Crawler/seed-auth" \
    --data-raw '{"code":"TEST"}') || {
    echo "[seed] verify-voter (TEST) failed" >&2; exit 4;
  }
  TYPE=$(echo "$RESP" | python3 -c 'import sys, json; print(json.load(sys.stdin).get("type", "?"))')
  HASH=$(echo "$RESP" | python3 -c 'import sys, json; print(json.load(sys.stdin).get("voterHash", ""))')

  python3 - "$OUT" "$HASH" <<'PY'
import json, os, sys
out, voter_hash = sys.argv[1:3]
seed = {
    "role": "voter",
    "voterCode": "TEST",
    "voterHash": voter_hash,
    "autoGatekeeper": True,
    "createdAt": __import__("datetime").datetime.utcnow().isoformat() + "Z",
    "ttlNote": "TEST mode is built into the server — no expiry",
}
with open(out, "w") as f:
    json.dump(seed, f, indent=2)
os.chmod(out, 0o600)
PY

  echo "[seed] voter seed written to $OUT (type=$TYPE)"
fi

echo "[seed] now run:  node --loader ts-node/esm src/main.ts --journey journeys/sacred-vote-$ROLE.json"
