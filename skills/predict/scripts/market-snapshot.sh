#!/bin/sh
# One-shot market state for a token: everything needed to quote an order.
# Usage: market-snapshot.sh <TOKEN_ID> [tenant-host]
# Tenant resolution: $2 if given, else PM_TENANT / PM_CLOB_ENDPOINT from the environment.
set -e

TOKEN="${1:-}"
TENANT="${2:-}"

if [ -z "$TOKEN" ]; then
  echo "usage: $0 <TOKEN_ID> [tenant-host]" >&2
  exit 1
fi

run() {
  if [ -n "$TENANT" ]; then
    predict-cli --tenant "$TENANT" "$@"
  else
    predict-cli "$@"
  fi
}

echo "── tick-size ───────────────────────────"
run tick-size "$TOKEN"
echo "── fee-rate ────────────────────────────"
run fee-rate "$TOKEN"
echo "── midpoint ────────────────────────────"
run midpoint "$TOKEN"
echo "── spread ──────────────────────────────"
run spread "$TOKEN"
echo "── last-trade ──────────────────────────"
run last-trade "$TOKEN"
echo "── book ────────────────────────────────"
run book "$TOKEN"
