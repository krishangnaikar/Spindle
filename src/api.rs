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
    let app = Router::new()
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({"service":"spindle-api","protocol":1})) }),
        )
        .route("/v1/command", post(command))
        .layer(DefaultBodyLimit::max(crate::model::FRAME_LIMIT))
        .with_state(Api {
            dir: dir.to_owned(),
            permits: Arc::new(Semaphore::new(64)),
        });
    let listener = BoundedListener {
        inner: listener,
        permits: Arc::new(Semaphore::new(CONNECTION_LIMIT)),
    };
    axum::serve(listener, app).await?;
    Ok(())
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
