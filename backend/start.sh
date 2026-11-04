#!/bin/bash
set -euo pipefail
PUBLIC_PORT="\${PORT:-8080}"
WISP_PORT=8081
export HTTP_PROXY_PORT=8082
case "$PUBLIC_PORT" in
  *[!0-9]*|'') echo "PORT must be a numeric TCP port" >&2; exit 1 ;;
esac
sed "s/__PORT__/$WISP_PORT/g" /app/epoxy.toml > /tmp/epoxy.toml
/usr/local/bin/epoxy-server /tmp/epoxy.toml &
EPOXY_PID=$!
node /app/http-proxy/server.mjs &
PROXY_PID=$!
PORT="$PUBLIC_PORT" caddy run --config /etc/caddy/Caddyfile --adapter caddyfile &
CADDY_PID=$!
cleanup() { kill "$CADDY_PID" "$PROXY_PID" "$EPOXY_PID" 2>/dev/null || true; }
trap cleanup EXIT INT TERM
set +e
wait -n "$EPOXY_PID" "$PROXY_PID" "$CADDY_PID"
STATUS=$?
set -e
exit "$STATUS"
