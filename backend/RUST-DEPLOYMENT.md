# Rust-only Wispbyte deployment

This deployment path is intended for a Wispbyte Rust runtime that clones this repository and runs a custom startup command. It does not use the GHCR image or require Node.js/Caddy.

## Repository settings

- Git repository: https://github.com/sigmapoopyman69/ebertNet
- Branch: rust-only-backend
- Runtime image: rust_latest (or the current Rust image)
- Startup command: bash backend/start-rust.sh

The startup script builds the official Epoxy server from its multiplexed branch, starts it on local port 8081, then builds and starts the Rust HTTP/Wisp gateway on the assigned public PORT.

## Endpoints

- GET /health — gateway health response
- /proxy?url=https%3A%2F%2Fexample.com%2F — streaming HTTP proxy
- WebSocket requests on the public service — relayed to the local Epoxy/Wisp server

## Important

The Rust gateway is a transport foundation, not a complete transparent browser proxy. HTML/CSS rewriting, module URL resolution, cookie-domain translation, and origin-sensitive YouTube behavior still require separate frontend integration.

Do not expose this as an unrestricted public relay. The gateway only permits HTTP(S), standard ports, and public destination IPs, and pins DNS resolutions to the addresses validated for each request.
