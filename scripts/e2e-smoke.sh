#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${MARKET_E2E_BASE_URL:-http://127.0.0.1:8080}"

curl_json() {
  curl -fsS "$@" | jq .
}

echo "healthz"
curl_json "$BASE_URL/v1/healthz"

echo "version"
curl_json "$BASE_URL/v1/version"

echo "public info"
curl_json "$BASE_URL/v1/public/info"

echo "prices"
curl_json "$BASE_URL/v1/prices"

echo "metrics"
curl_json "$BASE_URL/v1/metrics"

echo "static routes"
curl -fsS "$BASE_URL/dashboard/" >/dev/null
curl -fsS "$BASE_URL/claim/" >/dev/null
curl -fsS "$BASE_URL/support/" >/dev/null

echo "ok"
