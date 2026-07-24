#!/usr/bin/env bash
#
# Proves the Redis-backed GCRA rate limiter shares ONE limit across replicas.
#
# Brings up two application replicas (backend-a, backend-b) pointed at the
# same Redis instance, hammers BOTH concurrently for a fixed duration well
# above their configured rate, and checks that the *combined* number of
# admitted (2xx) requests across both replicas matches the single configured
# limit — not double it, which is what an in-memory, per-process limiter
# would produce under the same load.
#
# Usage:
#   scripts/rate_limit_multi_replica_test.sh [duration_secs] [workers]
#
# Requires: docker, docker compose, curl. No k6/vegeta dependency — this uses
# plain concurrent curl workers so it runs anywhere Docker does.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

DURATION="${1:-30}"
WORKERS="${2:-20}"
DEMO_RPM="${DEMO_RPM:-120}"   # requests per 60s window (2 req/s)
DEMO_BURST="${DEMO_BURST:-20}"
COMPOSE_FILE="compose.rate-limit-demo.yml"
PROJECT="tipjar-rl-demo"
RESULTS_DIR="$(mktemp -d)"

export DEMO_RPM DEMO_BURST

cleanup() {
  echo "--- Tearing down demo stack ---"
  docker compose -p "$PROJECT" -f "$COMPOSE_FILE" down -v --remove-orphans >/dev/null 2>&1 || true
  rm -rf "$RESULTS_DIR"
}
trap cleanup EXIT

echo "=== Rate limit multi-replica demo ==="
echo "Configured limit: ${DEMO_RPM} req/60s, burst ${DEMO_BURST} (shared across 2 replicas via Redis)"
echo "Load: ${WORKERS} concurrent workers hammering both replicas for ${DURATION}s"
echo

echo "--- Building and starting db, redis, backend-a, backend-b ---"
docker compose -p "$PROJECT" -f "$COMPOSE_FILE" up -d --build

echo "--- Waiting for both replicas to report healthy (first boot compiles the project, can take several minutes) ---"
for port in 18001 18002; do
  for _ in $(seq 1 180); do
    if curl -sf "http://localhost:${port}/api/v1/health" >/dev/null 2>&1; then
      echo "replica on :${port} is up"
      break
    fi
    sleep 5
  done
done

echo "--- Running load for ${DURATION}s ---"
start_ts=$(date +%s)

worker() {
  local id="$1"
  local outfile="${RESULTS_DIR}/worker_${id}.codes"
  : > "$outfile"
  local end=$(( $(date +%s) + DURATION ))
  local toggle=0
  while [ "$(date +%s)" -lt "$end" ]; do
    if [ $((toggle % 2)) -eq 0 ]; then
      port=18001
    else
      port=18002
    fi
    code=$(curl -s -o /dev/null -w '%{http_code}' "http://localhost:${port}/api/v1/health" || echo "000")
    echo "$code" >> "$outfile"
    toggle=$((toggle + 1))
  done
}

pids=()
for i in $(seq 1 "$WORKERS"); do
  worker "$i" &
  pids+=("$!")
done
for pid in "${pids[@]}"; do
  wait "$pid"
done

elapsed=$(( $(date +%s) - start_ts ))

cat "${RESULTS_DIR}"/worker_*.codes > "${RESULTS_DIR}/all.codes"
total=$(wc -l < "${RESULTS_DIR}/all.codes" | tr -d ' ')
admitted=$(grep -c '^200$' "${RESULTS_DIR}/all.codes" || true)
throttled=$(grep -c '^429$' "${RESULTS_DIR}/all.codes" || true)
other=$(( total - admitted - throttled ))

# Expected admitted count for a *single shared* limiter over the test window:
# burst + steady-state trickle for the remaining time.
rate_per_sec=$(awk -v rpm="$DEMO_RPM" 'BEGIN { print rpm / 60 }')
expected=$(awk -v burst="$DEMO_BURST" -v rate="$rate_per_sec" -v secs="$elapsed" 'BEGIN { print burst + (rate * secs) }')
tolerance=$(awk -v exp="$expected" 'BEGIN { print exp * 0.30 }')
lower=$(awk -v e="$expected" -v t="$tolerance" 'BEGIN { print e - t }')
upper=$(awk -v e="$expected" -v t="$tolerance" 'BEGIN { print e + t }')
# A per-replica (unshared) limiter would admit roughly double this.
unshared_expected=$(awk -v e="$expected" 'BEGIN { print e * 2 }')

echo
echo "=== Results (elapsed ${elapsed}s) ==="
echo "Total requests sent:      ${total}"
echo "Admitted (2xx):            ${admitted}"
echo "Throttled (429):            ${throttled}"
echo "Other/errors:                ${other}"
echo "Expected admitted if SHARED (single limiter): ~$(printf '%.0f' "$expected")"
echo "Expected admitted if UNSHARED (per-replica):  ~$(printf '%.0f' "$unshared_expected")"
echo

if awk -v a="$admitted" -v lo="$lower" -v hi="$upper" 'BEGIN { exit !(a >= lo && a <= hi) }'; then
  echo "PASS: combined admitted count (${admitted}) matches the single shared limit (expected ~$(printf '%.0f' "$expected"), tolerance +/-30%)."
  echo "This confirms both replicas are enforcing ONE Redis-backed limit, not one each."
  exit 0
else
  echo "FAIL: combined admitted count (${admitted}) is outside the expected shared-limit range [$(printf '%.0f' "$lower"), $(printf '%.0f' "$upper")]."
  echo "If it is close to the UNSHARED estimate above, the two replicas are not sharing state."
  exit 1
fi
