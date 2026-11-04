import http from "node:http";
import dns from "node:dns";
import net from "node:net";
import { Transform, Readable } from "node:stream";
import { Agent, fetch } from "undici";

const PORT = Number(process.env.HTTP_PROXY_PORT || 8082);
const MAX_UPLOAD_BYTES = 10 * 1024 * 1024;
const METHODS = new Set(["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"]);
const REDIRECTS = new Set([301, 302, 303, 307, 308]);

function publicIPv4(address) {
  const p = address.split(".").map(Number);
  if (p.length !== 4 || p.some(n => !Number.isInteger(n) || n < 0 || n > 255)) return false;
  const [a, b, c] = p;
  if (a === 0 || a === 10 || a === 127 || a >= 224) return false;
  if (a === 100 && b >= 64 && b <= 127) return false;
  if (a === 169 && b === 254) return false;
  if (a === 172 && b >= 16 && b <= 31) return false;
  if (a === 192 && (b === 0 || b === 168)) return false;
  if (a === 198 && (b === 18 || b === 19 || (b === 51 && c === 100))) return false;
  if (a === 203 && b === 0 && c === 113) return false;
  return true;
}
function publicIPv6(address) {
  const v = address.toLowerCase().split("%")[0];
  if (v === "::" || v === "::1" || /^(fc|fd|fe[89ab]|ff)/.test(v)) return false;
  const mapped = v.match(/::ffff:(\d+\.\d+\.\d+\.\d+)$/);
  if (mapped) return publicIPv4(mapped[1]);
  return true;
}
function publicIP(address) {
  const family = net.isIP(address);
  return family === 4 ? publicIPv4(address) : family === 6 ? publicIPv6(address) : false;
}
const safeLookup = (hostname, options, callback) => {
  dns.promises.lookup(hostname, { all: true, verbatim: true }).then(records => {
    if (!records.length || records.some(r => !publicIP(r.address))) throw new Error("Destination resolved to a non-public IP address.");
    if (options?.all) callback(null, records);
    else callback(null, records[0].address, records[0].family);
  }).catch(callback);
};
const dispatcher = new Agent({ connect: { lookup: safeLookup, timeout: 15000 }, headersTimeout: 30000, bodyTimeout: 0 });

function parseTarget(raw) {
  let target;
  try { target = new URL(raw); } catch { throw new Error("A valid absolute target URL is required."); }
  if (!["http:", "https:"].includes(target.protocol)) throw new Error("Only HTTP and HTTPS are supported.");
  if (target.username || target.password) throw new Error("Credentials in target URLs are not supported.");
  if (target.port && !["80", "443"].includes(target.port)) throw new Error("Only standard web ports are supported.");
  if (net.isIP(target.hostname) && !publicIP(target.hostname)) throw new Error("Private or special-use IP destinations are not allowed.");
  target.hash = "";
  return target;
}
function requestHeaders(req) {
  const headers = new Headers();
  for (const name of ["accept", "accept-language", "authorization", "cache-control", "content-type", "cookie", "if-match", "if-modified-since", "if-none-match", "if-range", "if-unmodified-since", "origin", "range", "referer", "user-agent"]) {
    if (typeof req.headers[name] === "string") headers.set(name, req.headers[name]);
  }
  return headers;
}
function responseHeaders(upstream, res, target) {
  for (const name of ["accept-ranges", "cache-control", "content-disposition", "content-language", "content-length", "content-range", "content-type", "etag", "expires", "last-modified", "vary"]) {
    const value = upstream.headers.get(name);
    if (value !== null) res.setHeader(name, value);
  }
  if (typeof upstream.headers.getSetCookie === "function") {
    const cookies = upstream.headers.getSetCookie();
    if (cookies.length) res.setHeader("set-cookie", cookies);
  }
  const location = upstream.headers.get("location");
  if (location) res.setHeader("location", "/proxy?url=" + encodeURIComponent(new URL(location, target).href));
}

const server = http.createServer(async (req, res) => {
  res.setHeader("x-content-type-options", "nosniff");
  // Browser pages run in a sandboxed origin and call this gateway cross-origin.
  // Wildcard CORS is intentional: this endpoint does not use browser credentials.
  res.setHeader("access-control-allow-origin", "*");
  res.setHeader("access-control-allow-methods", "GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS");
  res.setHeader("access-control-allow-headers", "accept, accept-language, authorization, cache-control, content-type, if-match, if-modified-since, if-none-match, if-range, if-unmodified-since, range");
  res.setHeader("access-control-expose-headers", "accept-ranges, content-range, content-length, content-type, etag, last-modified, location");
  if (req.method === "OPTIONS") {
    res.writeHead(204, { "cache-control": "no-store" });
    res.end();
    return;
  }
  if (req.url === "/health") {
    res.writeHead(200, { "content-type": "application/json; charset=utf-8", "cache-control": "no-store" });
    res.end(JSON.stringify({ service: "ebertNet HTTP proxy", status: "ok", streaming: true, rangeRequests: true }));
    return;
  }
  const incoming = new URL(req.url || "/", "http://localhost");
  if (incoming.pathname !== "/proxy") {
    res.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
    res.end("Use /proxy?url=https%3A%2F%2Fexample.com%2F");
    return;
  }
  if (!METHODS.has(req.method || "GET")) {
    res.writeHead(405, { allow: [...METHODS].join(", ") });
    res.end("Method not supported.");
    return;
  }
  let target;
  try {
    const raw = incoming.searchParams.get("url");
    if (!raw) throw new Error("Missing url query parameter.");
    target = parseTarget(raw);
  } catch (error) {
    res.writeHead(400, { "content-type": "text/plain; charset=utf-8" });
    res.end(error.message);
    return;
  }
  if (Number(req.headers["content-length"] || 0) > MAX_UPLOAD_BYTES) {
    res.writeHead(413); res.end("Request body exceeds the 10 MiB limit."); return;
  }
  const controller = new AbortController();
  req.on("aborted", () => controller.abort());
  res.on("close", () => { if (!res.writableEnded) controller.abort(); });
  try {
    let body;
    if (!["GET", "HEAD"].includes(req.method || "GET")) {
      const limiter = new Transform({
        transform(chunk, encoding, callback) {
          this.bytes = (this.bytes || 0) + chunk.length;
          if (this.bytes > MAX_UPLOAD_BYTES) callback(new Error("Request body exceeds the 10 MiB limit."));
          else callback(null, chunk);
        }
      });
      body = Readable.toWeb(req.pipe(limiter));
    }
    const upstream = await fetch(target, {
      method: req.method, headers: requestHeaders(req), body,
      duplex: body ? "half" : undefined, redirect: "manual",
      dispatcher, signal: controller.signal
    });
    res.statusCode = upstream.status;
    responseHeaders(upstream, res, target);
    if (REDIRECTS.has(upstream.status) && !res.hasHeader("location")) {
      const location = upstream.headers.get("location");
      if (location) res.setHeader("location", "/proxy?url=" + encodeURIComponent(new URL(location, target).href));
    }
    if (req.method === "HEAD" || !upstream.body) { res.end(); return; }
    Readable.fromWeb(upstream.body).on("error", error => {
      if (!res.headersSent) res.writeHead(502);
      res.destroy(error);
    }).pipe(res);
  } catch (error) {
    if (res.headersSent) { res.destroy(error); return; }
    res.writeHead(error?.name === "AbortError" ? 499 : 502, { "content-type": "text/plain; charset=utf-8", "cache-control": "no-store" });
    res.end("Upstream request failed: " + (error?.message || "unknown error"));
  }
});
server.headersTimeout = 30000;
server.requestTimeout = 0;
server.listen(PORT, "127.0.0.1", () => console.log("ebertNet HTTP proxy listening on 127.0.0.1:" + PORT));
