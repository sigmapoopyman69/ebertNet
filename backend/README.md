# ebertNet backend

The container runs three components behind one public service port:

- Epoxy/Wisp handles the existing secure WebSocket transport.
- The ebertNet HTTP proxy streams HTTP responses, including byte-range media responses.
- Caddy routes WebSocket upgrades to Epoxy and ordinary HTTP requests to the HTTP proxy.

## Deployment

Use a container host that supports Dockerfile deployments, WebSocket upgrades, and an externally assigned TCP port in the PORT environment variable. Set the build context to the repository root and Dockerfile path to backend/Dockerfile. The public service must be available over HTTPS/WSS.

The existing Wisp endpoint remains the public service root, for example wss://YOUR-SERVICE-HOST/.

The HTTP proxy health endpoint is https://YOUR-SERVICE-HOST/health. A raw request can be tested at https://YOUR-SERVICE-HOST/proxy?url=https%3A%2F%2Fexample.com%2F.

## HTTP proxy behavior

This first implementation:
- streams upstream response bodies rather than buffering entire files;
- forwards byte-range and conditional request headers;
- preserves status codes and relevant media headers, including Content-Range, Accept-Ranges, and Content-Type;
- supports GET, HEAD, POST, PUT, PATCH, DELETE, and OPTIONS;
- rewrites upstream redirect locations back through the proxy;
- limits request bodies to 10 MiB;
- rejects non-HTTP(S) targets, non-standard ports, URL credentials, and private/special-use IP destinations;
- validates DNS answers and pins outbound connections to validated public addresses.

This is the transport foundation, not yet a complete transparent browser proxy. HTML and CSS URL rewriting, JavaScript module URL resolution, browser fetch/XHR integration, cookie-domain translation, and YouTube-specific origin behavior still need frontend/proxy integration. The existing iframe srcdoc renderer does not gain those capabilities merely because this endpoint exists.

## Security

Do not deploy this service as an unrestricted public relay for other users. Keep it private to its intended user, add authentication and rate limits before sharing it, and retain the destination and network-scope checks.

The Epoxy/Wisp server retains its existing stream policy: public DNS hostnames, standard web ports, and no direct IP, loopback, multicast, or non-global destinations.
