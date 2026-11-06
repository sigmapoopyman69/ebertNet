use axum::{
    body::{to_bytes, Body},
    extract::{
        ws::{Message as AxumMessage, WebSocket, WebSocketUpgrade},
        OriginalUri, State,
    },
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use reqwest::{redirect::Policy, Client};
use std::{net::{IpAddr, SocketAddr}, time::Duration};
use tokio::net::lookup_host;
use tokio_tungstenite::{connect_async, tungstenite::{client::IntoClientRequest, Message as TungsteniteMessage}};
use url::Url;

const MAX_UPLOAD_BYTES: usize = 10 * 1024 * 1024;
const EPOXY_ADDR: &str = "ws://127.0.0.1:8081";

#[derive(Clone)]
struct AppState {
    port: u16,
}

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(8080);
    let app = Router::new()
        .route("/health", get(health))
        .route("/proxy", any(proxy))
        .fallback(any(wisp))
        .with_state(AppState { port });

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("could not bind public listener");
    eprintln!("ebertNet Rust gateway listening on 0.0.0.0:{port}");
    axum::serve(listener, app).await.expect("gateway server failed");
}

async fn health() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        r#"{"service":"ebertNet Rust HTTP/Wisp gateway","status":"ok","streaming":true,"rangeRequests":true}"#,
    )
}

async fn proxy(State(_state): State<AppState>, request: Request<Body>) -> Response {
    let mut response = Response::new(Body::empty());
    add_cors(response.headers_mut());

    if request.method() == Method::OPTIONS {
        *response.status_mut() = StatusCode::NO_CONTENT;
        response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return response;
    }

    let target = match target_from_request(&request) {
        Ok(url) => url,
        Err(message) => return text_response(StatusCode::BAD_REQUEST, message),
    };

    let method = request.method().clone();
    let incoming_headers = request.headers().clone();
    let body = match to_bytes(request.into_body(), MAX_UPLOAD_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => return text_response(StatusCode::PAYLOAD_TOO_LARGE, "Request body exceeds the 10 MiB limit."),
    };

    let client = match safe_client(&target).await {
        Ok(client) => client,
        Err(message) => return text_response(StatusCode::BAD_GATEWAY, message),
    };

    let mut headers = reqwest::header::HeaderMap::new();
    // Forward only end-to-end headers needed by normal web pages and media.
    for name in [
        "accept", "accept-language", "authorization", "cache-control", "content-type",
        "cookie", "if-match", "if-modified-since", "if-none-match", "if-range",
        "if-unmodified-since", "origin", "range", "referer", "user-agent",
    ] {
        if let Some(value) = incoming_headers.get(name) {
            headers.insert(name, value.clone());
        }
    }

    let mut builder = client.request(method.clone(), target.clone()).headers(headers);
    if method != Method::GET && method != Method::HEAD && !body.is_empty() {
        builder = builder.body(body);
    }

    let upstream = match builder.send().await {
        Ok(response) => response,
        Err(error) => return text_response(StatusCode::BAD_GATEWAY, &format!("Upstream request failed: {error}")),
    };

    let status = upstream.status();
    let upstream_headers = upstream.headers().clone();
    let mut out = Response::new(Body::from_stream(upstream.bytes_stream()));
    *out.status_mut() = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    add_cors(out.headers_mut());

    for name in [
        header::ACCEPT_RANGES, header::CACHE_CONTROL, header::CONTENT_DISPOSITION,
        header::CONTENT_LANGUAGE, header::CONTENT_LENGTH, header::CONTENT_RANGE,
        header::CONTENT_TYPE, header::ETAG, header::EXPIRES, header::LAST_MODIFIED,
        header::VARY,
    ] {
        if let Some(value) = upstream_headers.get(&name) {
            out.headers_mut().insert(name, value.clone());
        }
    }

    if let Some(location) = upstream_headers.get(header::LOCATION).and_then(|v| v.to_str().ok()) {
        if let Ok(resolved) = target.join(location) {
            let rewritten = format!("/proxy?url={}", url::form_urlencoded::byte_serialize(resolved.as_str().as_bytes()).collect::<String>());
            if let Ok(value) = HeaderValue::from_str(&rewritten) {
                out.headers_mut().insert(header::LOCATION, value);
            }
        }
    }

    if method == Method::HEAD {
        *out.body_mut() = Body::empty();
    }
    out
}

fn target_from_request(request: &Request<Body>) -> Result<Url, &'static str> {
    let query = request.uri().query().ok_or("Missing url query parameter.")?;
    let raw = url::form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == "url")
        .map(|(_, value)| value.into_owned())
        .ok_or("Missing url query parameter.")?;
    let target = Url::parse(&raw).map_err(|_| "A valid absolute target URL is required.")?;
    if !matches!(target.scheme(), "http" | "https") {
        return Err("Only HTTP and HTTPS are supported.");
    }
    if !target.username().is_empty() || target.password().is_some() {
        return Err("Credentials in target URLs are not supported.");
    }
    if target.port().is_some_and(|port| port != 80 && port != 443) {
        return Err("Only standard web ports are supported.");
    }
    if let Some(host) = target.host_str() {
        let normalized = host.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = normalized.parse::<IpAddr>() {
            if !is_public_ip(ip) {
                return Err("Private or special-use IP destinations are not allowed.");
            }
        }
    }
    Ok(target)
}

async fn safe_client(target: &Url) -> Result<Client, &'static str> {
    let host = target.host_str().ok_or("Target URL has no hostname.")?;
    let port = target.port_or_known_default().ok_or("Target URL has no port.")?;
    let normalized = host.trim_start_matches('[').trim_end_matches(']');
    let addresses: Vec<SocketAddr> = if let Ok(ip) = normalized.parse::<IpAddr>() {
        if !is_public_ip(ip) {
            return Err("Private or special-use IP destinations are not allowed.");
        }
        vec![SocketAddr::new(ip, port)]
    } else {
        let resolved: Vec<SocketAddr> = lookup_host((host, port)).await
            .map_err(|_| "Could not resolve target hostname.")?
            .collect();
        if resolved.is_empty() || resolved.iter().any(|address| !is_public_ip(address.ip())) {
            return Err("Target hostname resolved to a private or special-use IP address.");
        }
        resolved
    };

    let mut builder = Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::ZERO);
    if normalized.parse::<IpAddr>().is_err() {
        builder = builder.resolve_to_addrs(host, &addresses);
    }
    builder.build().map_err(|_| "Could not create upstream HTTP client.")
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0 || a == 10 || a == 127 || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && (b == 0 || b == 168))
                || (a == 192 && b == 0 && c == 2)
                || (a == 192 && b == 88 && c == 99)
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            let mapped = ip.to_ipv4_mapped();
            if let Some(v4) = mapped {
                return is_public_ip(IpAddr::V4(v4));
            }
            !(ip.is_unspecified() || ip.is_loopback()
                || (segments[0] & 0xfe00) == 0xfc00 // unique-local
                || (segments[0] & 0xffc0) == 0xfe80 // link-local
                || (segments[0] & 0xff00) == 0xff00) // multicast
        }
    }
}

async fn wisp(
    ws: WebSocketUpgrade,
    OriginalUri(uri): OriginalUri,
) -> impl IntoResponse {
    let path = uri.path_and_query().map(|value| value.as_str()).unwrap_or("/");
    let protocol = ws.protocol().map(str::to_owned);
    ws.on_upgrade(move |socket| relay_wisp(socket, path.to_owned(), protocol))
}

async fn relay_wisp(mut downstream: WebSocket, path: String, protocol: Option<String>) {
    let upstream_url = format!("{EPOXY_ADDR}{path}");
    let mut request = match upstream_url.into_client_request() {
        Ok(request) => request,
        Err(error) => {
            eprintln!("Wisp upstream request error: {error}");
            let _ = downstream.close().await;
            return;
        }
    };
    if let Some(protocol) = protocol {
        if let Ok(value) = HeaderValue::from_str(&protocol) {
            request.headers_mut().insert(header::SEC_WEBSOCKET_PROTOCOL, value);
        }
    }

    let (upstream, _) = match connect_async(request).await {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Could not connect to local Epoxy Wisp server: {error}");
            let _ = downstream.close().await;
            return;
        }
    };
    let (mut up_sink, mut up_stream) = upstream.split();
    let (mut down_sink, mut down_stream) = downstream.split();

    loop {
        tokio::select! {
            message = down_stream.next() => {
                match message {
                    Some(Ok(message)) => {
                        let converted = match message {
                            AxumMessage::Text(text) => TungsteniteMessage::Text(text.to_string().into()),
                            AxumMessage::Binary(bytes) => TungsteniteMessage::Binary(bytes),
                            AxumMessage::Ping(bytes) => TungsteniteMessage::Ping(bytes),
                            AxumMessage::Pong(bytes) => TungsteniteMessage::Pong(bytes),
                            AxumMessage::Close(_) => { let _ = up_sink.send(TungsteniteMessage::Close(None)).await; break; }
                        };
                        if up_sink.send(converted).await.is_err() { break; }
                    }
                    _ => break,
                }
            }
            message = up_stream.next() => {
                match message {
                    Some(Ok(message)) => {
                        let converted = match message {
                            TungsteniteMessage::Text(text) => AxumMessage::Text(text.to_string().into()),
                            TungsteniteMessage::Binary(bytes) => AxumMessage::Binary(bytes),
                            TungsteniteMessage::Ping(bytes) => AxumMessage::Ping(bytes),
                            TungsteniteMessage::Pong(bytes) => AxumMessage::Pong(bytes),
                            TungsteniteMessage::Close(_) => { let _ = down_sink.send(AxumMessage::Close(None)).await; break; }
                            TungsteniteMessage::Frame(_) => continue,
                        };
                        if down_sink.send(converted).await.is_err() { break; }
                    }
                    _ => break,
                }
            }
        }
    }
}

fn add_cors(headers: &mut HeaderMap) {
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS"));
    headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("accept, accept-language, authorization, cache-control, content-type, if-match, if-modified-since, if-none-match, if-range, if-unmodified-since, range"));
    headers.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, HeaderValue::from_static("accept-ranges, content-range, content-length, content-type, etag, last-modified, location"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
}

fn text_response(status: StatusCode, message: &str) -> Response {
    let mut response = (status, message.to_owned()).into_response();
    add_cors(response.headers_mut());
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
    response
}
