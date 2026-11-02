# ebertNet Epoxy/Wisp backend

This directory contains a container recipe for the official Rust Epoxy Wisp server.

## Deployment

Use a container hosting service that supports:
- Dockerfile-based deployments
- WebSocket upgrades
- An externally assigned TCP port exposed through the `PORT` environment variable
- HTTPS/WSS at the public service URL

Set the Docker build context to the repository root so the Dockerfile can copy `backend/epoxy.toml` and `backend/start.sh`. Set the Dockerfile path to `backend/Dockerfile`.

After deployment, the Wisp endpoint is the service's secure WebSocket URL (for example, `wss://YOUR-SERVICE-HOST/`). Do not put a real backend secret in this public repository.

## Important safety defaults

The starter config is deliberately not an unrestricted public relay:
- Only `example.com` and `developer.mozilla.org` are allowed initially.
- Only ports 80 and 443 are allowed.
- UDP, direct IP connections, loopback, multicast, and non-global destinations are disabled.

Keep this allowlist while testing. Do not widen it into an unrestricted public relay. Before adding more destinations or allowing other users, add an authentication design supported by the exact Epoxy client version and review hosting-provider access controls.

## Upstream

The server is built from the `server/` crate in the official MercuryWorkshop/epoxy-tls repository, branch `multiplexed`. The upstream project requires Rust nightly.
