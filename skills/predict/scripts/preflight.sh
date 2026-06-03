#!/bin/sh
# Trading-session readiness check: server, endpoints, wallet, balance.
# Usage: preflight.sh [tenant-host]
# Tenant resolution: $1 if given, else PM_TENANT / PM_CLOB_ENDPOINT from the environment.
set -e

TENANT="${1:-}"

run() {
  if [ -n "$TENANT" ]; then
    predict-cli --tenant "$TENANT" "$@"
  else
    predict-cli "$@"
  fi
}

echo "── server ──────────────────────────────"
run ok
run time

echo "── endpoints ───────────────────────────"
run endpoints

echo "── wallet ──────────────────────────────"
predict-cli wallet show || echo "no wallet configured — run: predict-cli setup"

echo "── collateral balance ──────────────────"
run balance --asset-type collateral || \
  echo "balance unavailable — needs an L2 key (predict-cli auth derive-key) and a configured wallet"

echo "── preflight done ──────────────────────"
