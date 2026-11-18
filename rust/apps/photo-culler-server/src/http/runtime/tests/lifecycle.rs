use std::{io, net::SocketAddr};

use axum::{Router, routing::get as get_route};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use super::{start, start_and_announce};

async fn http_get(addr: SocketAddr) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

#[tokio::test]
async fn assigned_port_is_announced_after_router_is_live_and_shutdown_releases_it() {
    let mut output = Vec::new();
    let server = start_and_announce(
        0,
        |_| Ok::<_, &'static str>(Router::new().route("/", get_route(|| async { "ok" }))),
        &mut output,
    )
    .await
    .unwrap();
    let ready: serde_json::Value = serde_json::from_slice(&output[..output.len() - 1]).unwrap();
    assert_eq!(ready["event"], "ready");
    assert_eq!(ready["protocol"], 1);
    let origin = ready["origin"].as_str().unwrap();
    assert!(origin.starts_with("http://127.0.0.1:") && !origin.ends_with(":0"));
    assert!(http_get(server.address()).await.contains("200 OK"));
    let address = server.address();
    server.shutdown().await.unwrap();
    assert!(TcpStream::connect(address).await.is_err());
}

#[tokio::test]
async fn eof_and_independent_signal_both_release_listener() {
    for eof_mode in [true, false] {
        let server = start(0, |_| Ok::<_, &'static str>(Router::new()))
            .await
            .unwrap();
        let addr = server.address();
        let (eof_tx, eof_rx) = tokio::sync::oneshot::channel();
        let (signal_tx, signal_rx) = tokio::sync::oneshot::channel();
        let waiter = tokio::spawn(server.wait_for_shutdown(
            async move {
                let _ = eof_rx.await;
            },
            async move {
                let _ = signal_rx.await;
            },
        ));
        if eof_mode {
            let _ = eof_tx.send(());
        } else {
            let _ = signal_tx.send(());
        }
        waiter.await.unwrap().unwrap();
        assert!(TcpStream::connect(addr).await.is_err());
    }
}

#[tokio::test]
async fn preparation_failure_never_emits_ready() {
    let mut output = Vec::new();
    let result =
        start_and_announce(0, |_| Err::<Router, _>("fixture unavailable"), &mut output).await;
    assert!(result.is_err());
    assert!(output.is_empty());
}

#[tokio::test]
async fn bind_failure_never_emits_ready() {
    let occupied = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = occupied.local_addr().unwrap().port();
    let mut output = Vec::new();
    let result = start_and_announce(port, |_| Ok::<_, io::Error>(Router::new()), &mut output).await;
    assert!(result.is_err());
    assert!(output.is_empty());
}
