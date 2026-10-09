//! End-to-end proxy tests: real sockets, real HTTP, real encryption.
//! Everything binds to 127.0.0.1 ephemeral ports; no external network.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use brera_core::proxy::{spawn_proxy, ProxyConfig};

async fn spawn_target(response: &'static [u8]) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind target");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                // read the request head (one read is enough for tests)
                let _ = sock.read(&mut buf).await;
                let _ = sock.write_all(response).await;
            });
        }
    });
    addr
}

async fn spawn_echo() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind echo");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                loop {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if sock.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
    });
    addr
}

#[tokio::test]
async fn absolute_form_get_roundtrip_through_channel() {
    let target = spawn_target(
        b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\nx-brera: probe\r\ncontent-length: 5\r\n\r\nping!",
        )
        .await;

    let server = spawn_proxy(ProxyConfig::default()).await.expect("proxy");
    let proxy_addr = server.local_addr;

    let mut client = TcpStream::connect(proxy_addr).await.expect("connect");
    let req = format!(
        "GET http://{}/probe HTTP/1.1\r\nHost: {target}\r\nx-trace: e2e-1\r\nConnection: close\r\n\r\n",
        target
    );
    client.write_all(req.as_bytes()).await.expect("write");
    let mut raw = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut raw))
        .await
        .expect("no timeout")
        .expect("read");

    let text = String::from_utf8_lossy(&raw);
    assert!(text.starts_with("HTTP/1.1 200"), "got: {text}");
    assert!(text.contains("ping!"), "body must survive the channel");
    assert!(text.contains("x-brera: probe"), "custom header preserved");
    assert!(!text.contains("transfer-encoding"), "hop-by-hop stripped");

    let snap = server.snapshot();
    assert_eq!(snap.requests, 1);
    assert!(snap.bytes_in > 0);
    assert!(snap.mode.contains("loopback"));
    server.stop().await;
}

#[tokio::test]
async fn post_body_flows_intact() {
    // target echoes the request body back in a header-free simple response
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let target = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let Ok((mut sock, _)) = listener.accept().await else {
            return;
        };
        // single read: the whole request fits one buffer; the pooled client
        // never half-closes its write side, so read_to_end would deadlock
        let mut buf = [0u8; 4096];
        let _ = sock.read(&mut buf).await;
        let text = String::from_utf8_lossy(&buf);
        let body = text.split("\r\n\r\n").nth(1).unwrap_or("");
        let resp = format!(
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = sock.write_all(resp.as_bytes()).await;
    });

    let server = spawn_proxy(ProxyConfig::default()).await.expect("proxy");
    let mut client = TcpStream::connect(server.local_addr)
        .await
        .expect("connect");
    let payload = "tx=0x1&value=42&memo=breraos-e2e";
    let req = format!(
        "POST http://{}/submit HTTP/1.1\r\nHost: {target}\r\ncontent-type: application/x-www-form-urlencoded\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        target,
        payload.len(),
        payload
    );
    client.write_all(req.as_bytes()).await.expect("write");
    let mut raw = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut raw))
        .await
        .expect("no timeout")
        .expect("read");
    let text = String::from_utf8_lossy(&raw);
    assert!(text.contains(payload), "POST body must round-trip: {text}");
    server.stop().await;
}

#[tokio::test]
async fn connect_tunnels_to_echo_server() {
    let echo = spawn_echo().await;
    let server = spawn_proxy(ProxyConfig::default()).await.expect("proxy");

    let mut client = TcpStream::connect(server.local_addr)
        .await
        .expect("connect");
    let req = format!("CONNECT {echo} HTTP/1.1\r\nHost: {echo}\r\n\r\n");
    client.write_all(req.as_bytes()).await.expect("write");

    // read the 200 head
    let mut head = [0u8; 512];
    let n = tokio::time::timeout(Duration::from_secs(5), client.read(&mut head))
        .await
        .expect("no timeout")
        .expect("read head");
    let head_text = String::from_utf8_lossy(&head[..n]);
    assert!(
        head_text.starts_with("HTTP/1.1 200"),
        "CONNECT must be accepted: {head_text}"
    );

    // tunnel is now transparent: send, expect echo
    client.write_all(b"tunnel-payload-42").await.expect("write");
    let mut buf = [0u8; 64];
    let n = tokio::time::timeout(Duration::from_secs(5), client.read(&mut buf))
        .await
        .expect("no timeout")
        .expect("echo read");
    assert_eq!(&buf[..n], b"tunnel-payload-42");

    let snap = server.snapshot();
    assert_eq!(snap.tunnels, 1);
    server.stop().await;
}

#[tokio::test]
async fn origin_form_is_rejected() {
    let server = spawn_proxy(ProxyConfig::default()).await.expect("proxy");
    let mut client = TcpStream::connect(server.local_addr)
        .await
        .expect("connect");
    client
        .write_all(b"GET /local HTTP/1.1\r\nHost: localhost\r\nconnection: close\r\n\r\n")
        .await
        .expect("write");
    let mut raw = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut raw))
        .await
        .expect("no timeout")
        .expect("read");
    let text = String::from_utf8_lossy(&raw);
    assert!(text.contains("400"), "origin-form must be refused: {text}");
    assert_eq!(server.snapshot().errors, 1);
    server.stop().await;
}

#[tokio::test]
async fn refuses_non_loopback_bind() {
    let cfg = ProxyConfig {
        bind_ip: Some(std::net::IpAddr::from([0, 0, 0, 0])),
        ..Default::default()
    };
    let err = match spawn_proxy(cfg).await {
        Err(e) => e,
        Ok(_) => panic!("must refuse 0.0.0.0"),
    };
    assert!(err.to_string().contains("loopback"));
}

// keep std io import used even if tests shift around
