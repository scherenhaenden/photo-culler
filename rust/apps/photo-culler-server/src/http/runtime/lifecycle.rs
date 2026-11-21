//! Loopback HTTP lifecycle for the read-only Rust/Slint demo.
//!
//! D01/D02 are still open on the sprint base. This module follows their accepted
//! JSON-line `ready` message and injects the router after the assigned port is known.

use std::{
    future::Future,
    io::{self, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use axum::Router;
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

#[derive(Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReadyMessage {
    pub event: &'static str,
    pub protocol: u8,
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
}

impl std::fmt::Debug for ReadyMessage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReadyMessage")
            .field("event", &self.event)
            .field("protocol", &self.protocol)
            .field("origin", &self.origin)
            .field(
                "session_token",
                &self.session_token.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

#[derive(Debug)]
pub struct LifecycleError(String);
impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for LifecycleError {}
impl From<io::Error> for LifecycleError {
    fn from(value: io::Error) -> Self {
        Self(value.to_string())
    }
}

pub struct RunningServer {
    address: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<io::Result<()>>,
}
impl RunningServer {
    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn ready_message(&self, session_token: Option<String>) -> ReadyMessage {
        ReadyMessage {
            event: "ready",
            protocol: 1,
            origin: format!("http://{}", self.address),
            session_token,
        }
    }
    pub async fn shutdown(mut self) -> io::Result<()> {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        self.task.await.map_err(io::Error::other)?
    }
    pub async fn wait_for_shutdown<E, S>(self, eof: E, signal: S) -> io::Result<()>
    where
        E: Future<Output = ()>,
        S: Future<Output = ()>,
    {
        tokio::select! { _ = eof => {}, _ = signal => {} }
        self.shutdown().await
    }
}

/// Bind loopback first, then construct the router with the effective address.
/// Preparation errors occur before `ready` can be emitted.
pub async fn start<F, E>(port: u16, make_router: F) -> Result<RunningServer, LifecycleError>
where
    F: FnOnce(SocketAddr) -> Result<Router, E>,
    E: std::fmt::Display,
{
    start_on(IpAddr::V4(Ipv4Addr::LOCALHOST), port, make_router).await
}

/// Start on the requested loopback address. Non-loopback binds are rejected
/// here as well as by the demo CLI so future callers cannot expose this server.
pub async fn start_on<F, E>(
    bind_ip: IpAddr,
    port: u16,
    make_router: F,
) -> Result<RunningServer, LifecycleError>
where
    F: FnOnce(SocketAddr) -> Result<Router, E>,
    E: std::fmt::Display,
{
    if !bind_ip.is_loopback() {
        return Err(LifecycleError(
            "server bind address must be loopback".to_owned(),
        ));
    }
    let listener = TcpListener::bind(SocketAddr::new(bind_ip, port)).await?;
    let address = listener.local_addr()?;
    let app = make_router(address)
        .map_err(|e| LifecycleError(format!("router preparation failed: {e}")))?;
    let (shutdown, rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
    });
    Ok(RunningServer {
        address,
        shutdown: Some(shutdown),
        task,
    })
}

pub async fn start_and_announce<F, E, W>(
    port: u16,
    make_router: F,
    output: &mut W,
) -> Result<RunningServer, LifecycleError>
where
    F: FnOnce(SocketAddr) -> Result<Router, E>,
    E: std::fmt::Display,
    W: Write,
{
    let server = start(port, make_router).await?;
    let mut line = serde_json::to_vec(&server.ready_message(None))
        .map_err(|e| LifecycleError(e.to_string()))?;
    line.push(b'\n');
    if let Err(error) = output.write_all(&line) {
        let _ = server.shutdown().await;
        return Err(error.into());
    }
    Ok(server)
}

#[cfg(test)]
#[path = "tests/lifecycle.rs"]
mod tests;
