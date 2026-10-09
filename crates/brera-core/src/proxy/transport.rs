//! Outbound transports behind one enum. The proxy speaks to transports;
//! transports speak to the world (or, today, refuse to).
//!
//! - `Loopback`: real TCP egress via a hyper client (and raw TCP for tunnels).
//! - `P2pStub`: the placeholder for the future peer-routed channel. Same
//!   signature, deliberately fails closed — never silently degrades.

use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt as _, Full};
use hyper::body::Incoming;
use hyper::{Method, Request, StatusCode, Version};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::CoreError;
use crate::proxy::channel::{Frame, SessionCipher};
use crate::CoreResult;

type FullBody = Full<Bytes>;

/// Which transport the proxy should route through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportMode {
    Loopback,
    P2p,
}

impl TransportMode {
    pub fn label(self) -> &'static str {
        match self {
            TransportMode::Loopback => "loopback-direct",
            TransportMode::P2p => "p2p-stub",
        }
    }
}

/// The transport surface. One enum keeps `Send` bounds trivial and the
/// dispatch table flat; new modes are new arms, not new object graphs.
pub struct Transport {
    mode: TransportMode,
    client: Client<HttpConnector, FullBody>,
    pub(crate) cipher: Arc<SessionCipher>,
}

impl Transport {
    pub fn new(mode: TransportMode, cipher: Arc<SessionCipher>) -> Self {
        let client = Client::builder(TokioExecutor::new()).build_http::<FullBody>();
        Self {
            mode,
            client,
            cipher,
        }
    }

    pub fn mode(&self) -> TransportMode {
        self.mode
    }

    /// Carry a sealed request frame to the route, return the sealed
    /// response frame. The channel encryption wraps the *whole* exchange,
    /// whatever the underlying mode ends up being.
    pub async fn exchange(&self, frame: Frame, route: &Route) -> CoreResult<Frame> {
        match self.mode {
            TransportMode::Loopback => {
                // open locally (we terminate the channel), perform real HTTP,
                // seal the answer back
                let plaintext = self.cipher.open(&frame)?;
                let wire = decode_wire_request(&plaintext)?;
                let resp = self.direct_http(&wire, route).await?;
                let sealed = encode_wire_response(&resp)?;
                Ok(self.cipher.seal(&sealed))
            }
            TransportMode::P2p => Err(CoreError::Transport(
                "p2p channel not wired yet: frames are not routed to peers in this build".into(),
            )),
        }
    }

    /// Raw TCP tunnel target for CONNECT. Loopback dials directly; P2p refuses.
    pub async fn tunnel(&self, route: &Route) -> CoreResult<TcpStream> {
        match self.mode {
            TransportMode::Loopback => TcpStream::connect((route.host.as_str(), route.port))
                .await
                .map_err(|e| CoreError::Transport(format!("dial {route}: {e}"))),
            TransportMode::P2p => Err(CoreError::Transport("p2p tunnels not wired yet".into())),
        }
    }

    async fn direct_http(&self, wire: &WireRequest, route: &Route) -> CoreResult<WireResponse> {
        let mut builder = Request::builder()
            .method(
                Method::from_bytes(wire.method.as_bytes())
                    .map_err(|_| CoreError::Frame(format!("bad method {}", wire.method)))?,
            )
            .uri(&wire.uri)
            .version(Version::HTTP_11);
        for (k, v) in &wire.headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        let req = builder
            .body(FullBody::from(Bytes::from(wire.body.clone())))
            .map_err(|e| CoreError::Proxy(format!("request build failed: {e}")))?;

        let resp = self
            .client
            .request(req)
            .await
            .map_err(|e| CoreError::Transport(format!("egress failed for {route}: {e}")))?;

        let (parts, body) = resp.into_parts();
        let body = read_limited(body, MAX_BODY).await?;
        Ok(WireResponse {
            status: parts.status.as_u16(),
            headers: parts
                .headers
                .iter()
                .map(|(k, v)| {
                    (
                        k.as_str().to_string(),
                        String::from_utf8_lossy(v.as_bytes()).to_string(),
                    )
                })
                .collect(),
            body,
        })
    }
}

/// Where a frame is headed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Route {
    pub host: String,
    pub port: u16,
}

impl std::fmt::Display for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

pub const MAX_BODY: usize = 8 * 1024 * 1024;

/// Wire form of an intercepted request (inside the encrypted channel).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WireRequest {
    pub method: String,
    pub uri: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Wire form of the response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WireResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub(crate) fn encode_wire_request(w: &WireRequest) -> CoreResult<Vec<u8>> {
    serde_json::to_vec(w).map_err(|e| CoreError::Frame(format!("wire encode: {e}")))
}

fn decode_wire_request(bytes: &[u8]) -> CoreResult<WireRequest> {
    serde_json::from_slice(bytes).map_err(|e| CoreError::Frame(format!("wire decode: {e}")))
}

fn encode_wire_response(w: &WireResponse) -> CoreResult<Vec<u8>> {
    serde_json::to_vec(w).map_err(|e| CoreError::Frame(format!("wire encode: {e}")))
}

pub(crate) async fn read_limited(mut body: Incoming, limit: usize) -> CoreResult<Vec<u8>> {
    let mut out = Vec::new();
    while let Some(chunk) = body.frame().await {
        let data = chunk
            .map_err(|e| CoreError::Proxy(format!("body read failed: {e}")))?
            .into_data()
            .map_err(|_| CoreError::Proxy("unexpected frame trailers".into()))?;
        if out.len() + data.len() > limit {
            return Err(CoreError::PayloadTooLarge { limit });
        }
        out.extend_from_slice(&data);
    }
    Ok(out)
}

/// Helper used by the proxy server: build a `WireResponse` straight from a
/// status (used for error pages the proxy itself generates).
pub(crate) fn plain_response(status: StatusCode, text: &str) -> WireResponse {
    WireResponse {
        status: status.as_u16(),
        headers: vec![(
            "content-type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        )],
        body: text.as_bytes().to_vec(),
    }
}

/// Pipe two full-duplex streams into each other until either side closes.
pub(crate) async fn pipe_bidirectional<A, B>(a: A, b: B) -> CoreResult<()>
where
    A: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    B: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut ar, mut aw) = tokio::io::split(a);
    let (mut br, mut bw) = tokio::io::split(b);
    let up = async move {
        let mut buf = [0u8; 8192];
        loop {
            let n = ar.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            aw.write_all(&buf[..n]).await?;
        }
        let _ = aw.shutdown().await;
        CoreResult::Ok(())
    };
    let down = async move {
        let mut buf = [0u8; 8192];
        loop {
            let n = br.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            bw.write_all(&buf[..n]).await?;
        }
        let _ = bw.shutdown().await;
        CoreResult::Ok(())
    };
    let (a, b) = tokio::join!(up, down);
    a?;
    b?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_request_json_roundtrip() {
        let w = WireRequest {
            method: "GET".into(),
            uri: "http://example.test/x".into(),
            headers: vec![("host".into(), "example.test".into())],
            body: vec![1, 2, 3],
        };
        let enc = encode_wire_request(&w).expect("encode");
        let back = decode_wire_request(&enc).expect("decode");
        assert_eq!(back.uri, w.uri);
        assert_eq!(back.body, vec![1, 2, 3]);
    }

    #[test]
    fn p2p_stub_fails_closed() {
        let cipher = Arc::new(SessionCipher::new());
        let t = Transport::new(TransportMode::P2p, cipher);
        let frame = crate::proxy::channel::SessionCipher::new().seal(b"x");
        let route = Route {
            host: "example.test".into(),
            port: 80,
        };
        let rt = tokio::runtime::Runtime::new().expect("rt");
        let err = rt
            .block_on(t.exchange(frame, &route))
            .expect_err("stub must fail closed");
        assert!(matches!(err, CoreError::Transport(_)));
    }

    #[test]
    fn route_display() {
        let r = Route {
            host: "h".into(),
            port: 1,
        };
        assert_eq!(r.to_string(), "h:1");
    }
}
