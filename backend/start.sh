#!/bin/sh
set -eu
PORT_VALUE="${PORT:-8080}"
case "$PORT_VALUE" in
  *[!0-9]*|'') echo "PORT must be a numeric TCP port" >&2; exit 1 ;;
esac
sed "s/__PORT__/$PORT_VALUE/g" /app/epoxy.toml > /tmp/epoxy.toml
exec /usr/local/bin/epoxy-server /tmp/epoxy.toml
