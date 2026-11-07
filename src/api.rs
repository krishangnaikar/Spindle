//! Unix-socket HTTP gateway. No TCP/browser exposure in this milestone.
use crate::{
    host,
    model::{Reply, Request},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    routing::{get, post},
};
use std::{
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{UnixListener, UnixStream, unix::SocketAddr},
    sync::{OwnedSemaphorePermit, Semaphore},
    time::Sleep,
};

const CONNECTION_LIMIT: usize = 64;
const CONNECTION_LIFETIME: Duration = Duration::from_secs(10);

// Admission happens before HTTP parsing, so idle or slow-header/body clients
// consume the same bounded connection budget as fully decoded commands.
struct BoundedListener {
    inner: UnixListener,
    permits: Arc<Semaphore>,
}

impl axum::serve::Listener for BoundedListener {
    type Io = BoundedConnection;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            // Delegate accept-error handling/backoff to axum's Unix listener.
            let (stream, addr) = axum::serve::Listener::accept(&mut self.inner).await;
            let Ok(permit) = self.permits.clone().try_acquire_owned() else {
                drop(stream);
                tokio::task::yield_now().await;
                continue;
            };
            return (
                BoundedConnection {
                    stream,
                    deadline: Box::pin(tokio::time::sleep(CONNECTION_LIFETIME)),
                    _permit: permit,
                },
                addr,
            );
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

// The permit belongs to the transport, not an individual keep-alive request.
// There are no streaming endpoints: the deadline is an absolute connection
// lifetime, never refreshed by a client trickling another byte.
struct BoundedConnection {
    stream: UnixStream,
    deadline: Pin<Box<Sleep>>,
    _permit: OwnedSemaphorePermit,
}

impl BoundedConnection {
    fn check_deadline(&mut self, cx: &mut Context<'_>) -> io::Result<()> {
        if self.deadline.as_mut().poll(cx).is_ready() {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "gateway connection lifetime exceeded",
            ))
        } else {
            Ok(())
        }
    }
}

impl AsyncRead for BoundedConnection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if let Err(error) = this.check_deadline(cx) {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut this.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for BoundedConnection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if let Err(error) = this.check_deadline(cx) {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut this.stream).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if let Err(error) = this.check_deadline(cx) {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut this.stream).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if let Err(error) = this.check_deadline(cx) {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut this.stream).poll_shutdown(cx)
    }
}

#[derive(Clone)]
struct Api {
    dir: PathBuf,
    permits: Arc<Semaphore>,
}
pub async fn run(dir: &Path) -> anyhow::Result<()> {
    let (listener, _guard) = host::bind_socket(dir, "api")?;
    let app = router(dir);
    let listener = BoundedListener {
        inner: listener,
        permits: Arc::new(Semaphore::new(CONNECTION_LIMIT)),
    };
    axum::serve(listener, app).await?;
    Ok(())
}

fn router(dir: &Path) -> Router {
    Router::new()
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({"service":"spindle-api","protocol":1})) }),
        )
        .route("/v1/command", post(command))
        .layer(DefaultBodyLimit::max(crate::model::FRAME_LIMIT))
        .with_state(Api {
            dir: dir.to_owned(),
            permits: Arc::new(Semaphore::new(64)),
        })
}
async fn command(State(api): State<Api>, Json(request): Json<Request>) -> Json<Reply> {
    let Ok(_permit) = api.permits.try_acquire() else {
        return Json(Reply::from_result(Err(anyhow::anyhow!("gateway busy"))));
    };
    Json(match host::request(&api.dir, &request).await {
        Ok(reply) => reply,
        Err(e) => Reply::from_result(Err(e)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request as HttpRequest, StatusCode},
    };
    use http_body_util::BodyExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tower::ServiceExt;

    // Socket pairs exercise the transport adapter without spawning a host.
    fn connection() -> (BoundedConnection, UnixStream, Arc<Semaphore>) {
        let (stream, peer) = UnixStream::pair().unwrap();
        let permits = Arc::new(Semaphore::new(1));
        let permit = permits.clone().try_acquire_owned().unwrap();
        (
            BoundedConnection {
                stream,
                deadline: Box::pin(tokio::time::sleep(CONNECTION_LIFETIME)),
                _permit: permit,
            },
            peer,
            permits,
        )
    }

    #[tokio::test]
    async fn transport_forwards_bytes_flush_shutdown_and_releases_permit() {
        let (mut io, mut peer, permits) = connection();
        assert_eq!(permits.available_permits(), 0);
        peer.write_all(b"request").await.unwrap();
        let mut received = [0; 7];
        io.read_exact(&mut received).await.unwrap();
        assert_eq!(&received, b"request");
        io.write_all(b"reply").await.unwrap();
        io.flush().await.unwrap();
        let mut reply = [0; 5];
        peer.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"reply");
        io.shutdown().await.unwrap();
        assert_eq!(peer.read(&mut [0; 1]).await.unwrap(), 0);
        // Half-close is not transport ownership release.
        assert_eq!(permits.available_permits(), 0);
        drop(io);
        assert_eq!(permits.available_permits(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn pending_read_is_woken_by_absolute_deadline() {
        let (mut io, _peer, permits) = connection();
        let start = tokio::time::Instant::now();
        let error = io.read(&mut [0; 1]).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(start.elapsed(), CONNECTION_LIFETIME);
        assert_eq!(permits.available_permits(), 0);
        drop(io);
        assert_eq!(permits.available_permits(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn all_io_operations_refuse_after_expiration() {
        let (mut io, _peer, _) = connection();
        tokio::time::advance(CONNECTION_LIFETIME).await;
        assert_eq!(
            io.read(&mut [0; 1]).await.unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            io.write(b"late").await.unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            io.flush().await.unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            io.shutdown().await.unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }

    #[tokio::test(start_paused = true)]
    async fn successful_io_does_not_extend_connection_lifetime() {
        let (mut io, mut peer, _) = connection();
        tokio::time::advance(Duration::from_secs(9)).await;
        // Writing demonstrates progress without depending on paused-clock
        // readiness scheduling for a read from an external socket.
        io.write_all(b"x").await.unwrap();
        let mut byte = [0; 1];
        peer.read_exact(&mut byte).await.unwrap();
        assert_eq!(byte, [b'x']);
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(
            io.write(b"y").await.unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }

    #[tokio::test]
    async fn listener_drops_overflow_and_admits_after_transport_drop() {
        use axum::serve::Listener;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("listener.sock");
        let permits = Arc::new(Semaphore::new(1));
        let mut listener = BoundedListener {
            inner: UnixListener::bind(&path).unwrap(),
            permits: permits.clone(),
        };
        assert_eq!(
            listener.local_addr().unwrap().as_pathname(),
            Some(path.as_path())
        );
        let _first_client = UnixStream::connect(&path).await.unwrap();
        let (first, _) = listener.accept().await;
        assert_eq!(permits.available_permits(), 0);
        let accepting = tokio::spawn(async move { listener.accept().await.0 });
        let mut overflow = UnixStream::connect(&path).await.unwrap();
        let n = tokio::time::timeout(Duration::from_secs(2), overflow.read(&mut [0; 1]))
            .await
            .expect("overflow must close promptly")
            .unwrap();
        assert_eq!(n, 0);
        assert!(!accepting.is_finished());
        drop(first);
        let mut next_client = UnixStream::connect(&path).await.unwrap();
        let mut next = tokio::time::timeout(Duration::from_secs(2), accepting)
            .await
            .unwrap()
            .unwrap();
        next_client.write_all(b"n").await.unwrap();
        let mut byte = [0; 1];
        next.read_exact(&mut byte).await.unwrap();
        assert_eq!(byte, [b'n']);
        assert_eq!(permits.available_permits(), 0);
        drop(next);
        assert_eq!(permits.available_permits(), 1);
    }

    #[tokio::test]
    async fn command_reports_host_failure_and_releases_request_slot() {
        let dir = tempfile::tempdir().unwrap();
        let permits = Arc::new(Semaphore::new(1));
        let Json(reply) = command(
            State(Api {
                dir: dir.path().to_owned(),
                permits: permits.clone(),
            }),
            Json(Request::Health {}),
        )
        .await;
        assert!(!reply.ok);
        assert!(reply.error.is_some());
        assert!(reply.value.is_none());
        assert_eq!(permits.available_permits(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn blocked_writer_is_woken_by_deadline() {
        let (mut io, _peer, _) = connection();
        // Fill the socket send buffer while its peer never drains it.
        let error = io.write_all(&vec![0; 4 * 1024 * 1024]).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[tokio::test]
    async fn command_preserves_host_success_and_domain_refusal() {
        let dir = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(dir.path().join("host.sock")).unwrap();
        let fake = tokio::spawn(async move {
            for reply in [
                r#"{"ok":true,"value":{"source":"fixture"}}"#,
                r#"{"ok":false,"error":"stale generation"}"#,
            ] {
                let (mut peer, _) = listener.accept().await.unwrap();
                let request = host::read_frame(&mut peer).await.unwrap();
                assert!(matches!(
                    serde_json::from_slice::<Request>(&request).unwrap(),
                    Request::Health {}
                ));
                peer.write_all(format!("{reply}\n").as_bytes())
                    .await
                    .unwrap();
            }
        });
        let api = Api {
            dir: dir.path().to_owned(),
            permits: Arc::new(Semaphore::new(1)),
        };
        let Json(success) = command(State(api.clone()), Json(Request::Health {})).await;
        assert!(success.ok);
        assert_eq!(success.value, Some(serde_json::json!({"source":"fixture"})));
        let Json(refusal) = command(State(api.clone()), Json(Request::Health {})).await;
        assert!(!refusal.ok);
        assert_eq!(refusal.error.as_deref(), Some("stale generation"));
        assert_eq!(api.permits.available_permits(), 1);
        fake.await.unwrap();
    }

    #[tokio::test]
    async fn saturated_command_fails_before_host_access() {
        let dir = tempfile::tempdir().unwrap();
        let permits = Arc::new(Semaphore::new(1));
        let _held = permits.acquire().await.unwrap();
        let Json(reply) = command(
            State(Api {
                dir: dir.path().to_owned(),
                permits: permits.clone(),
            }),
            Json(Request::Health {}),
        )
        .await;
        assert!(!reply.ok);
        assert_eq!(reply.error.as_deref(), Some("gateway busy"));
        assert_eq!(permits.available_permits(), 0);
    }

    async fn http(
        method: &str,
        path: &str,
        content_type: Option<&str>,
        body: String,
    ) -> (StatusCode, Vec<u8>) {
        let dir = tempfile::tempdir().unwrap();
        let mut req = HttpRequest::builder().method(method).uri(path);
        if let Some(content_type) = content_type {
            req = req.header("content-type", content_type);
        }
        let response = router(dir.path())
            .oneshot(req.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        (status, bytes)
    }

    #[tokio::test]
    async fn health_is_independent_of_host_availability() {
        let (status, bytes) = http("GET", "/health", None, String::new()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            serde_json::json!({"service":"spindle-api", "protocol":1})
        );
    }

    #[tokio::test]
    async fn router_refuses_invalid_json_schema_media_method_and_path() {
        for (body, status) in [
            ("{", StatusCode::BAD_REQUEST),
            (r#"{"op":"unknown"}"#, StatusCode::UNPROCESSABLE_ENTITY),
            (
                r#"{"op":"health","unexpected":true}"#,
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                r#"{"op":"list","unexpected":true}"#,
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (r#"{"op":"resize"}"#, StatusCode::UNPROCESSABLE_ENTITY),
        ] {
            assert_eq!(
                http(
                    "POST",
                    "/v1/command",
                    Some("application/json"),
                    body.to_owned()
                )
                .await
                .0,
                status,
                "request body: {body}"
            );
        }
        assert_eq!(
            http("POST", "/v1/command", None, "{}".into()).await.0,
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        assert_eq!(
            http("GET", "/v1/command", None, String::new()).await.0,
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            http("POST", "/health", None, String::new()).await.0,
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            http("GET", "/missing", None, String::new()).await.0,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn router_enforces_body_cap_before_command_execution() {
        let mut body = r#"{"op":"health"}"#.to_owned();
        body.push_str(&" ".repeat(crate::model::FRAME_LIMIT - body.len()));
        let (status, bytes) = http(
            "POST",
            "/v1/command",
            Some("application/json"),
            body.clone(),
        )
        .await;
        // Exactly the cap reaches the handler, which reports absent host.
        assert_eq!(status, StatusCode::OK);
        let reply: Reply = serde_json::from_slice(&bytes).unwrap();
        assert!(!reply.ok);
        body.push(' ');
        assert_eq!(
            http("POST", "/v1/command", Some("application/json"), body)
                .await
                .0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }
}
