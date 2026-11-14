#!/usr/bin/env bash
# External uptime & readiness probe for Nullbox.
# Checks liveness (/api/healthz) and database readiness (/api/readyz) and posts
# an alert to /api/alerts/webhook (and optional ALERT_WEBHOOK_URL) on failure.
set -euo pipefail

BASE_URL="${1:-https://team-09.fasthacks.cloud}"
BASE_URL="${BASE_URL%/}"
REQ_ID="uptime-$(date -u +%Y%m%dT%H%M%SZ)"

send_alert() {
    local title="$1"
    local message="$2"
    local payload
    payload=$(printf '{"title":"%s","message":"%s","severity":"critical","source":"external_uptime"}' "$title" "$message")
    curl -fsS -X POST "${BASE_URL}/api/alerts/webhook" \
        -H "Content-Type: application/json" \
        -H "X-Request-Id: ${REQ_ID}" \
        -d "${payload}" >/dev/null 2>&1 || true

    if [ -n "${ALERT_WEBHOOK_URL:-}" ]; then
        curl -fsS -X POST "${ALERT_WEBHOOK_URL}" \
            -H "Content-Type: application/json" \
            -d "${payload}" >/dev/null 2>&1 || true
    fi
}

if ! curl -fsS -m 5 -H "X-Request-Id: ${REQ_ID}" "${BASE_URL}/api/healthz" >/dev/null; then
    send_alert "Liveness check failed" "${BASE_URL}/api/healthz did not return 200 OK"
    exit 1
fi

if ! curl -fsS -m 5 -H "X-Request-Id: ${REQ_ID}" "${BASE_URL}/api/readyz" >/dev/null; then
    send_alert "Readiness check failed" "${BASE_URL}/api/readyz reported database unavailable"
    exit 1
fi

echo "OK: ${BASE_URL} healthz + readyz (${REQ_ID})"
