//! Native REST transport. Each blocking socket request runs on a worker thread
//! and wakes the Slint local executor when the bounded response is available.

use photo_culler_client::demo::{
    MAX_IMPORT_REQUEST_BYTES, RequestFuture, Response, Transport, TransportError,
};
use std::{
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex, mpsc},
    task::{Context, Poll, Waker},
    time::Duration,
};

#[derive(Clone)]
pub struct NativeDemoTransport {
    endpoint: Arc<Mutex<Option<SocketAddr>>>,
    session_token: Arc<Mutex<Option<String>>>,
}

impl NativeDemoTransport {
    pub fn new(
        endpoint: Arc<Mutex<Option<SocketAddr>>>,
        session_token: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            endpoint,
            session_token,
        }
    }
}

struct ThreadResponse {
    receiver: mpsc::Receiver<Result<Response, TransportError>>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl Future for ThreadResponse {
    type Output = Result<Response, TransportError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        match self.receiver.try_recv() {
            Ok(response) => Poll::Ready(response),
            Err(mpsc::TryRecvError::Disconnected) => Poll::Ready(Err(TransportError(
                "native HTTP worker stopped before returning a response".into(),
            ))),
            Err(mpsc::TryRecvError::Empty) => {
                if let Ok(mut waker) = self.waker.lock() {
                    *waker = Some(context.waker().clone());
                }
                match self.receiver.try_recv() {
                    Ok(response) => Poll::Ready(response),
                    Err(mpsc::TryRecvError::Disconnected) => Poll::Ready(Err(TransportError(
                        "native HTTP worker stopped before returning a response".into(),
                    ))),
                    Err(mpsc::TryRecvError::Empty) => Poll::Pending,
                }
            }
        }
    }
}

impl Transport for NativeDemoTransport {
    fn get<'a>(&'a self, path: &'a str, max_response_bytes: usize) -> RequestFuture<'a> {
        self.request(path, None, max_response_bytes)
    }

    fn put_json<'a>(
        &'a self,
        path: &'a str,
        body: Vec<u8>,
        max_response_bytes: usize,
    ) -> RequestFuture<'a> {
        self.request(path, Some(body), max_response_bytes)
    }

    fn post_json<'a>(
        &'a self,
        path: &'a str,
        body: Vec<u8>,
        max_response_bytes: usize,
    ) -> RequestFuture<'a> {
        if body.len() > MAX_IMPORT_REQUEST_BYTES {
            return Box::pin(async {
                Err(TransportError(format!(
                    "POST JSON body exceeds the {MAX_IMPORT_REQUEST_BYTES} byte limit"
                )))
            });
        }
        self.request_post(path, body, max_response_bytes)
    }
}

impl NativeDemoTransport {
    fn request<'a>(
        &'a self,
        path: &'a str,
        body: Option<Vec<u8>>,
        max_response_bytes: usize,
    ) -> RequestFuture<'a> {
        self.request_with_method(path, body, max_response_bytes, false)
    }

    fn request_post<'a>(
        &'a self,
        path: &'a str,
        body: Vec<u8>,
        max_response_bytes: usize,
    ) -> RequestFuture<'a> {
        self.request_with_method(path, Some(body), max_response_bytes, true)
    }

    fn request_with_method<'a>(
        &'a self,
        path: &'a str,
        body: Option<Vec<u8>>,
        max_response_bytes: usize,
        post: bool,
    ) -> RequestFuture<'a> {
        let endpoint = self.endpoint.lock().ok().and_then(|endpoint| *endpoint);
        let Some(endpoint) = endpoint else {
            return Box::pin(async {
                Err(TransportError(
                    "the local Photo Culler server is not available yet".into(),
                ))
            });
        };
        let session_token = match self.session_token.lock() {
            Ok(token) => token.clone(),
            Err(_) => {
                return Box::pin(async {
                    Err(TransportError(
                        "native session credential is unavailable".into(),
                    ))
                });
            }
        };
        let Some(session_token) = session_token else {
            return Box::pin(async {
                Err(TransportError(
                    "native session credential is required".into(),
                ))
            });
        };
        let path = path.to_owned();
        let (sender, receiver) = mpsc::channel();
        let waker = Arc::new(Mutex::new(None::<Waker>));
        let worker_waker = waker.clone();
        std::thread::spawn(move || {
            let response = crate::desktop::demo_http::NativeHttpTransport::new_authenticated(
                endpoint,
                Duration::from_secs(5),
                max_response_bytes,
                session_token,
            )
            .map_err(|error| TransportError(format!("native HTTP setup: {error:?}")))
            .and_then(|transport| {
                let response = match body {
                    Some(body) if post => transport.post_json(&path, &body),
                    Some(body) => transport.put_json(&path, &body),
                    None => transport.get(&path),
                };
                response.map_err(|error| {
                    let route = path.split('?').next().unwrap_or(&path);
                    TransportError(format!("native HTTP request to {route}: {error:?}"))
                })
            })
            .map(|response| Response {
                status: response.status,
                content_type: Some(response.content_type),
                body: response.body,
                redirected: false,
            });
            let _ = sender.send(response);
            if let Ok(mut saved) = worker_waker.lock() {
                if let Some(waker) = saved.take() {
                    waker.wake();
                }
            }
        });
        Box::pin(ThreadResponse { receiver, waker })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::Wake;

    struct NoopWake;
    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    #[test]
    fn rejects_requests_until_a_loopback_server_is_attached() {
        let transport =
            NativeDemoTransport::new(Arc::new(Mutex::new(None)), Arc::new(Mutex::new(None)));
        let mut future = transport.get("/api/v1/health", 1024);
        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(
            future.as_mut().poll(&mut context),
            Poll::Ready(Err(_))
        ));
    }
}
