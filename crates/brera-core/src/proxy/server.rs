//! The proxy server itself: accepts loopback connections, classifies each
//! request (absolute-form HTTP vs CONNECT), and pushes traffic through the
//! encrypted frame channel toward the active transport.
//!
//! Counters are atomic and always available; the UI polls a snapshot.

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::service::Service;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::error::CoreError;
use crate::proxy::channel::SessionCipher;
use crate::proxy::transport::{
    pipe_bidirectional, plain_response, read_limited, Route, Transport, TransportMode, WireRequest,
    WireResponse, MAX_BODY,
};
use crate::CoreResult;

/// Hop-by-hop headers never forwarded (RFC 7230 §6.1).
const HOP_BY_HOP: [&str; 9] = [
    "connection",
    "keep-alive",
    "proxy-connection",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Live counters, atomics only.
#[derive(Debug, Default)]
pub struct ProxyCounters {
    pub requests: AtomicU64,
    pub tunnels: AtomicU64,
    pub errors: AtomicU64,
    pub bytes_out: AtomicU64,
    pub bytes_in: AtomicU64,
}

impl ProxyCounters {
    pub fn snapshot(&self) -> ProxySnapshot {
        ProxySnapshot {
            running: true,
            addr: String::new(),
            mode: TransportMode::Loopback.label().to_string(),
            requests: self.requests.load(Ordering::Relaxed),
            tunnels: self.tunnels.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            bytes_out: self.bytes_out.load(Ordering::Relaxed),
            bytes_in: self.bytes_in.load(Ordering::Relaxed),
        }
    }

    /// Zero every counter atomically; returns the number of counters cleared.
    pub fn reset(&self) -> usize {
        self.requests.store(0, Ordering::Relaxed);
        self.tunnels.store(0, Ordering::Relaxed);
        self.errors.store(0, Ordering::Relaxed);
        self.bytes_out.store(0, Ordering::Relaxed);
        self.bytes_in.store(0, Ordering::Relaxed);
        5
    }
}

/// Immutable-ish status for the UI.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ProxySnapshot {
    pub running: bool,
    pub addr: String,
    pub mode: String,
    pub requests: u64,
    pub tunnels: u64,
    pub errors: u64,
    pub bytes_out: u64,
    pub bytes_in: u64,
}

/// Configuration accepted from the shell/UI.
#[derive(Debug, Clone, Copy)]
pub struct ProxyConfig {
    /// Transport mode (loopback implemented; p2p stub).
    pub mode: TransportMode,
    /// Bind IP; defaults to 127.0.0.1. Non-loopback binds are refused.
    pub bind_ip: Option<IpAddr>,
    /// Desired port; 0 or None picks an ephemeral one.
    pub port: Option<u16>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            mode: TransportMode::Loopback,
            bind_ip: None,
            port: None,
        }
    }
}

/// Control surface handed back to the shell.
pub struct ProxyServer {
    pub local_addr: SocketAddr,
    counters: Arc<ProxyCounters>,
    shutdown_tx: watch::Sender<bool>,
    join: Option<JoinHandle<()>>,
    started: Instant,
    mode: TransportMode,
}

impl ProxyServer {
    pub fn snapshot(&self) -> ProxySnapshot {
        let mut snap = self.counters.snapshot();
        snap.addr = self.local_addr.to_string();
        snap.mode = self.mode.label().to_string();
        snap.running = !self.shutdown_tx.is_closed();
        snap
    }

    /// Uptime in seconds (integer, for the UI).
    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Stop the accept loop and wait for it to finish.
    pub async fn stop(mut self) {
        let _ = self.shutdown_tx.send(true);
        if let Some(join) = self.join.take() {
            let _ = join.await;
        }
    }

    /// Wipe telemetry buffers and reset counters; returns how many counter
    /// groups were scrubbed (for the purge receipt).
    pub fn purge_scratch(&self) -> u64 {
        let wiped = self.counters.reset();
        wiped as u64
    }
}

impl Drop for ProxyServer {
    fn drop(&mut self) {
        // best-effort: signal the loop even if nobody awaited stop()
        let _ = self.shutdown_tx.send(true);
    }
}

/// Spawn the proxy on a loopback address. Refuses non-loopback binds.
pub async fn spawn_proxy(cfg: ProxyConfig) -> CoreResult<ProxyServer> {
    let ip = cfg.bind_ip.unwrap_or(IpAddr::from([127, 0, 0, 1]));
    if !ip.is_loopback() {
        return Err(CoreError::Proxy(
            "refusing to bind the intercept proxy off loopback".into(),
        ));
    }
    let port = cfg.port.unwrap_or(0);
    let listener = TcpListener::bind((ip, port))
        .await
        .map_err(|e| CoreError::Proxy(format!("bind failed: {e}")))?;
    let local_addr = listener.local_addr()?;

    let cipher = Arc::new(SessionCipher::new());
    let transport = Arc::new(Transport::new(cfg.mode, Arc::clone(&cipher)));
    let counters = Arc::new(ProxyCounters::default());
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let counters_task = Arc::clone(&counters);
    let join = tokio::spawn(async move {
        let mut shutdown_rx = shutdown_rx;
        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        break;
                    }
                }
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, _peer)) => {
                            let svc = ProxySvc {
                                transport: Arc::clone(&transport),
                                counters: Arc::clone(&counters_task),
                            };
                            tokio::spawn(async move {
                                let io = TokioIo::new(stream);
                                // with_upgrades: CONNECT tunnels rely on the upgrade
                                let conn = hyper::server::conn::http1::Builder::new()
                                    .serve_connection(io, svc)
                                    .with_upgrades();
                                if conn.await.is_err() {
                                    // connection-level failure: counted by the service
                                }
                            });
                        }
                        Err(_) => {
                            counters_task.errors.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    });

    Ok(ProxyServer {
        local_addr,
        counters,
        shutdown_tx,
        join: Some(join),
        started: Instant::now(),
        mode: cfg.mode,
    })
}

/// Per-connection service.
#[derive(Clone)]
struct ProxySvc {
    transport: Arc<Transport>,
    counters: Arc<ProxyCounters>,
}

type SvcResponse = Response<Full<Bytes>>;

impl Service<Request<Incoming>> for ProxySvc {
    type Response = SvcResponse;
    type Error = std::convert::Infallible;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<SvcResponse, Self::Error>> + Send>,
    >;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let transport = Arc::clone(&self.transport);
        let counters = Arc::clone(&self.counters);
        Box::pin(async move {
            let resp = handle(req, transport, counters).await;
            Ok(resp)
        })
    }
}

async fn handle(
    req: Request<Incoming>,
    transport: Arc<Transport>,
    counters: Arc<ProxyCounters>,
) -> SvcResponse {
    if req.method() == hyper::Method::CONNECT {
        return handle_connect(req, transport, counters).await;
    }
    handle_http(req, transport, counters).await
}

#[allow(clippy::too_many_lines)]
async fn handle_http(
    req: Request<Incoming>,
    transport: Arc<Transport>,
    counters: Arc<ProxyCounters>,
) -> SvcResponse {
    counters.requests.fetch_add(1, Ordering::Relaxed);

    // absolute-form or bust
    let uri = req.uri().clone();
    let (host, port) = match (uri.host(), uri.port_u16(), uri.scheme_str()) {
        (Some(h), p, Some("http")) => (h.to_string(), p.unwrap_or(80)),
        (Some(h), p, Some("https")) => {
            // plain absolute-form https is not a thing we do; CONNECT only
            counters.errors.fetch_add(1, Ordering::Relaxed);
            return error_response(
                StatusCode::BAD_GATEWAY,
                &format!(
                    "https absolute-form unsupported; use CONNECT ({h}:{})",
                    p.unwrap_or(443)
                ),
            );
        }
        _ => {
            counters.errors.fetch_add(1, Ordering::Relaxed);
            return error_response(
                StatusCode::BAD_REQUEST,
                "origin-form requests are not accepted; this endpoint expects absolute-form",
            );
        }
    };
    let route = Route { host, port };

    let method = req.method().to_string();
    let headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .filter(|(k, _)| {
            let name = k.as_str().to_ascii_lowercase();
            !HOP_BY_HOP.contains(&name.as_str()) && name != "content-length"
        })
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                String::from_utf8_lossy(v.as_bytes()).to_string(),
            )
        })
        .collect();
    let body = match read_limited(req.into_body(), MAX_BODY).await {
        Ok(b) => b,
        Err(e) => {
            counters.errors.fetch_add(1, Ordering::Relaxed);
            return match e {
                crate::CoreError::PayloadTooLarge { limit } => error_response(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    &format!("body exceeds proxy limit ({limit} bytes)"),
                ),
                other => error_response(StatusCode::BAD_REQUEST, &other.to_string()),
            };
        }
    };

    let wire = WireRequest {
        method,
        uri: uri.to_string(),
        headers,
        body,
    };

    // through the channel
    let frame = transport.cipher_seal(&wire);
    counters
        .bytes_out
        .fetch_add(wire.body.len() as u64, Ordering::Relaxed);

    let resp_frame = match transport.exchange(frame, &route).await {
        Ok(f) => f,
        Err(e) => {
            counters.errors.fetch_add(1, Ordering::Relaxed);
            return error_response(StatusCode::BAD_GATEWAY, &e.to_string());
        }
    };

    let wire_resp = match transport.cipher_open_response(resp_frame) {
        Ok(w) => w,
        Err(e) => {
            counters.errors.fetch_add(1, Ordering::Relaxed);
            return error_response(
                StatusCode::BAD_GATEWAY,
                &format!("channel open failed (tamper or desync): {e}"),
            );
        }
    };

    counters
        .bytes_in
        .fetch_add(wire_resp.body.len() as u64, Ordering::Relaxed);
    build_response(wire_resp)
}

async fn handle_connect(
    req: Request<Incoming>,
    transport: Arc<Transport>,
    counters: Arc<ProxyCounters>,
) -> SvcResponse {
    // authority-form: "host:port"
    let authority = req.uri().to_string();
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !h.is_empty() => {
            match p.parse::<u16>() {
                Ok(port) => (h.to_string(), port),
                Err(_) => {
                    return error_response(StatusCode::BAD_REQUEST, "bad CONNECT port");
                }
            }
        }
        _ => {
            counters.errors.fetch_add(1, Ordering::Relaxed);
            return error_response(StatusCode::BAD_REQUEST, "CONNECT expects host:port");
        }
    };
    let route = Route { host, port };

    // answer 200 immediately, then pipe the upgraded stream
    let response = Response::builder()
        .status(StatusCode::OK)
        .body(Full::new(Bytes::new()))
        .unwrap_or_else(|_| unreachable_response());

    let counters_task = Arc::clone(&counters);
    tokio::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                match transport.tunnel(&route).await {
                    Ok(target) => {
                        counters_task.tunnels.fetch_add(1, Ordering::Relaxed);
                        let _ = pipe_bidirectional(TokioIo::new(upgraded), target).await;
                    }
                    Err(_) => {
                        counters_task.errors.fetch_add(1, Ordering::Relaxed);
                        // the client sees a hang then a reset; that's the honest behavior
                    }
                }
            }
            Err(_) => {
                counters_task.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    response
}

fn build_response(w: WireResponse) -> SvcResponse {
    let status = StatusCode::from_u16(w.status).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut builder = Response::builder().status(status);
    for (k, v) in &w.headers {
        let name = k.to_ascii_lowercase();
        if HOP_BY_HOP.contains(&name.as_str()) || name == "content-length" {
            continue;
        }
        if let Ok(value) = v.parse::<hyper::header::HeaderValue>() {
            builder = builder.header(k.as_str(), value);
        }
    }
    builder
        .body(Full::new(Bytes::from(w.body)))
        .unwrap_or_else(|_| unreachable_response())
}

fn error_response(status: StatusCode, text: &str) -> SvcResponse {
    let wire = plain_response(status, text);
    build_response(wire)
}

fn unreachable_response() -> SvcResponse {
    // allocation failure territory; a fixed minimal response is still better
    // than a panic crossing the service boundary
    Response::new(Full::new(Bytes::from_static(
        b"breraos proxy: internal error",
    )))
}

/// Exposed for the shell: bytes flowed through the client connector of the
/// transport is not tracked separately today; the counters above are the
/// contract.
pub type SharedCounters = Arc<ProxyCounters>;

// The transport needs seal/open hooks used above; keep them thin so the
// channel stays the single crypto authority.
impl Transport {
    pub(crate) fn cipher_seal(&self, wire: &WireRequest) -> crate::proxy::channel::Frame {
        let bytes = super::transport::encode_wire_request(wire).unwrap_or_default();
        self.cipher.seal(&bytes)
    }

    pub(crate) fn cipher_open_response(
        &self,
        frame: crate::proxy::channel::Frame,
    ) -> CoreResult<WireResponse> {
        let bytes = self.cipher.open(&frame)?;
        serde_json::from_slice(&bytes).map_err(|e| CoreError::Frame(format!("wire decode: {e}")))
    }
}
