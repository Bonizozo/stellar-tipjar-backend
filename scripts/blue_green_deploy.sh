#!/usr/bin/env bash
# blue_green_deploy.sh — Blue-green deployment orchestration for stellar-tipjar-backend.
#
# Usage:
#   ./scripts/blue_green_deploy.sh deploy  <image-tag>   [--namespace <ns>]
#   ./scripts/blue_green_deploy.sh rollback               [--namespace <ns>]
#   ./scripts/blue_green_deploy.sh status                 [--namespace <ns>]
#
# Requirements: kubectl, curl
set -euo pipefail

# ── Defaults ──────────────────────────────────────────────────────────────────
NAMESPACE="${NAMESPACE:-default}"
APP="stellar-tipjar-backend"
HEALTH_TIMEOUT=120   # seconds to wait for standby to become ready
SMOKE_RETRIES=5
SMOKE_DELAY=5        # seconds between smoke-test retries
VS_NAME="$APP"       # Istio VirtualService name

# ── Helpers ───────────────────────────────────────────────────────────────────
log()  { echo "[$(date -u +%H:%M:%S)] $*"; }
die()  { echo "[ERROR] $*" >&2; exit 1; }

require() { command -v "$1" &>/dev/null || die "'$1' is required but not found."; }
require kubectl
require curl

# Parse --namespace flag anywhere in args
parse_args() {
    local args=()
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --namespace|-n) NAMESPACE="$2"; shift 2 ;;
            *) args+=("$1"); shift ;;
        esac
    done
    echo "${args[@]:-}"
}

# ── Live-slot detection ────────────────────────────────────────────────────────
# Reads the current default route subset from the Istio VirtualService.
get_live_slot() {
    kubectl get virtualservice "$VS_NAME" -n "$NAMESPACE" \
        -o jsonpath='{.spec.http[-1].route[0].destination.subset}' 2>/dev/null \
        || echo "blue"
}

get_standby_slot() {
    local live; live=$(get_live_slot)
    [[ "$live" == "blue" ]] && echo "green" || echo "blue"
}

# ── Traffic switch ─────────────────────────────────────────────────────────────
switch_traffic() {
    local target="$1"
    log "Switching VirtualService default route → $target"
    kubectl patch virtualservice "$VS_NAME" -n "$NAMESPACE" \
        --type=json \
        -p="[{\"op\":\"replace\",\"path\":\"/spec/http/2/route/0/destination/subset\",\"value\":\"${target}\"}]"
}

# ── Deployment update ──────────────────────────────────────────────────────────
update_image() {
    local slot="$1" tag="$2"
    log "Updating deployment-${slot} image to ${APP}:${tag}"
    kubectl set image deployment/"${APP}-${slot}" \
        "${APP}=${APP}:${tag}" \
        -n "$NAMESPACE"
}

# ── Readiness wait ─────────────────────────────────────────────────────────────
wait_ready() {
    local slot="$1"
    log "Waiting for deployment-${slot} to be ready (timeout: ${HEALTH_TIMEOUT}s)..."
    kubectl rollout status deployment/"${APP}-${slot}" \
        -n "$NAMESPACE" \
        --timeout="${HEALTH_TIMEOUT}s"
}

# ── Smoke test ─────────────────────────────────────────────────────────────────
# Hits /health on the standby slot via the header-based Istio route.
smoke_test() {
    local slot="$1"
    local svc_url
    svc_url=$(kubectl get svc "$APP" -n "$NAMESPACE" \
        -o jsonpath='http://{.spec.clusterIP}:{.spec.ports[0].port}' 2>/dev/null \
        || echo "http://${APP}.${NAMESPACE}.svc.cluster.local")

    log "Running smoke test against ${slot} slot (${svc_url}/health)..."
    local attempt=0
    while (( attempt < SMOKE_RETRIES )); do
        attempt=$(( attempt + 1 ))
        local http_code
        http_code=$(curl -sf -o /dev/null -w "%{http_code}" \
            -H "x-deployment-slot: ${slot}" \
            "${svc_url}/health" 2>/dev/null || echo "000")
        if [[ "$http_code" == "200" ]]; then
            log "Smoke test passed (attempt ${attempt})"
            return 0
        fi
        log "Smoke test attempt ${attempt}/${SMOKE_RETRIES} failed (HTTP ${http_code}), retrying in ${SMOKE_DELAY}s..."
        sleep "$SMOKE_DELAY"
    done
    return 1
}

# ── Commands ───────────────────────────────────────────────────────────────────
cmd_status() {
    local live; live=$(get_live_slot)
    local standby; standby=$(get_standby_slot)
    log "Live slot    : $live"
    log "Standby slot : $standby"
    echo ""
    kubectl get deployment \
        "${APP}-blue" "${APP}-green" \
        -n "$NAMESPACE" \
        --no-headers \
        -o custom-columns="NAME:.metadata.name,READY:.status.readyReplicas,DESIRED:.spec.replicas,IMAGE:.spec.template.spec.containers[0].image" \
        2>/dev/null || true
}

cmd_deploy() {
    local tag="${1:-}"
    [[ -z "$tag" ]] && die "Usage: $0 deploy <image-tag>"

    local standby; standby=$(get_standby_slot)
    local live;    live=$(get_live_slot)

    log "=== Blue-Green Deploy ==="
    log "Live slot    : $live"
    log "Standby slot : $standby"
    log "Image tag    : $tag"

    # 1. Push new image to standby slot.
    update_image "$standby" "$tag"

    # 2. Wait for standby pods to be ready.
    wait_ready "$standby"

    # 3. Smoke test the standby slot.
    if ! smoke_test "$standby"; then
        die "Smoke test failed for slot '${standby}'. Deployment aborted. Live slot '${live}' unchanged."
    fi

    # 4. Switch traffic.
    switch_traffic "$standby"
    log "=== Traffic switched to '${standby}' ==="

    # 5. Post-switch health check on the new live slot.
    sleep 3
    if ! smoke_test "$standby"; then
        log "Post-switch health check failed — initiating automatic rollback..."
        switch_traffic "$live"
        die "Rolled back to '${live}'. Investigate '${standby}' before retrying."
    fi

    log "=== Deployment complete. Live: ${standby} | Previous: ${live} ==="
}

cmd_rollback() {
    local live;    live=$(get_live_slot)
    local standby; standby=$(get_standby_slot)

    log "=== Rollback: ${live} → ${standby} ==="
    switch_traffic "$standby"
    log "Traffic switched back to '${standby}'."
    log "Run '$0 status' to verify."
}

# ── Entry point ────────────────────────────────────────────────────────────────
main() {
    local positional
    positional=$(parse_args "$@")
    # shellcheck disable=SC2086
    set -- $positional

    local cmd="${1:-status}"
    shift || true

    case "$cmd" in
        deploy)   cmd_deploy "$@" ;;
        rollback) cmd_rollback ;;
        status)   cmd_status ;;
        *) die "Unknown command '${cmd}'. Use: deploy <tag> | rollback | status" ;;
    esac
}

main "$@"
