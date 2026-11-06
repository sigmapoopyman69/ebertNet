#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT_VALUE="${PORT:-8080}"
WISP_PORT=8081
EPoxy_DIR="${TMPDIR:-/tmp}/ebertnet-epoxy-src"

case "$PORT_VALUE" in
  *[!0-9]*|'') echo "PORT must be a numeric TCP port" >&2; exit 1 ;;
esac

if [ ! -d "$EPoxy_DIR/.git" ]; then
  rm -rf "$EPoxy_DIR"
  git clone --depth 1 --branch multiplexed https://github.com/MercuryWorkshop/epoxy-tls.git "$EPoxy_DIR"
fi

echo "Building Epoxy/Wisp server..."
cargo build --release --manifest-path "$EPoxy_DIR/server/Cargo.toml"
sed "s/__PORT__/$WISP_PORT/g" "$ROOT/backend/epoxy.toml" > /tmp/ebertnet-epoxy.toml

"$EPoxy_DIR/target/release/epoxy-server" /tmp/ebertnet-epoxy.toml &
EPOXY_PID=$!
cleanup() {
  kill "$EPOXY_PID" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

cd "$ROOT"
echo "Building and starting ebertNet Rust gateway..."
PORT="$PORT_VALUE" cargo run --release --manifest-path backend/rust-gateway/Cargo.toml
