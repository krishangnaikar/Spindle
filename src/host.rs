//! Terminal ownership lives here, independently of the replaceable HTTP gateway.
use crate::{model::*, store::Store};
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde_json::{Value, json};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc as blocking,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{Mutex as AsyncMutex, Semaphore, oneshot},
};

pub fn private_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .context("create state directory (parent must exist)")?;
    }
    let meta = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "state directory must be a real directory"
    );
    // SAFETY: geteuid has no preconditions and does not access memory.
    anyhow::ensure!(
        meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0,
        "state directory must be owned by this user and mode 0700"
    );
    for name in ["state.db", "state.db-wal", "state.db-shm"] {
        if let Ok(meta) = std::fs::symlink_metadata(path.join(name)) {
            anyhow::ensure!(
                meta.is_file()
                    && !meta.file_type().is_symlink()
                    && meta.nlink() == 1
                    && meta.uid() == unsafe { libc::geteuid() },
                "unsafe state file"
            );
        }
    }
    Ok(())
}
pub struct SocketGuard {
    _lock: File,
    socket: PathBuf,
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket);
    }
}
pub fn bind_socket(dir: &Path, name: &str) -> Result<(UnixListener, SocketGuard)> {
    private_dir(dir)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(dir.join(format!("{name}.lock")))?;
    let metadata = lock.metadata()?;
    anyhow::ensure!(
        metadata.nlink() == 1 && metadata.uid() == unsafe { libc::geteuid() },
        "unsafe lock file"
    );
    lock.try_lock_exclusive()
        .context("another instance already owns this socket")?;
    let socket = dir.join(format!("{name}.sock"));
    if let Ok(meta) = std::fs::symlink_metadata(&socket) {
        anyhow::ensure!(
            meta.file_type().is_socket(),
            "refusing to remove a non-socket path"
        );
        std::fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    Ok((
        listener,
        SocketGuard {
            _lock: lock,
            socket,
        },
    ))
}

fn bounded_error(error: String) -> String {
    let mut text: String = error.chars().take(128).collect();
    // Bound encoded size too: control characters expand sixfold in JSON.
    while serde_json::to_vec(&text).map_or(true, |v| v.len() > 256) {
        text.pop();
    }
    text
}

type ControlReply = oneshot::Sender<Result<Value>>;
enum Control {
    Input(Vec<u8>, ControlReply),
    Resize(u16, u16, ControlReply),
    Cancel(ControlReply),
}
struct Live {
    generation: String,
    controls: blocking::SyncSender<Control>,
    ring: Arc<Mutex<Ring>>,
}
struct Host {
    store: Store,
    sessions: AsyncMutex<HashMap<String, Live>>,
    launches: AsyncMutex<()>,
    stopping: AtomicBool,
}

impl Host {
    async fn dispatch(&self, request: Request) -> Result<Value> {
        match request {
            Request::Health => Ok(json!({"service":"spindle-host","protocol":1})),
            Request::List => {
                let rows = self.store.list().await?;
                anyhow::ensure!(rows.len() <= 128, "record limit exceeded");
                let projected:Vec<_>=rows.into_iter().map(|s|json!({"id":s.launch.id,"generation":s.generation,"status":s.status,"exit_code":s.exit_code,"error":s.error.map(bounded_error)})).collect();
                Ok(json!(projected))
            }
            Request::Launch { launch } => self.launch(launch).await,
            Request::Output {
                id,
                generation,
                after,
            } => {
                let sessions = self.sessions.lock().await;
                let live = sessions
                    .get(&id)
                    .context("terminal not available in this host lifetime")?;
                anyhow::ensure!(live.generation == generation, "stale generation");
                Ok(serde_json::to_value(
                    live.ring.lock().unwrap().read(after)?,
                )?)
            }
            Request::Input {
                id,
                generation,
                data,
            } => {
                anyhow::ensure!(data.len() <= 16 * 1024, "input exceeds 16 KiB");
                self.control(id, generation, |reply| Control::Input(data, reply))
                    .await
            }
            Request::Resize {
                id,
                generation,
                rows,
                cols,
            } => {
                anyhow::ensure!(
                    (1..=1000).contains(&rows) && (1..=1000).contains(&cols),
                    "invalid terminal dimensions"
                );
                self.control(id, generation, |reply| Control::Resize(rows, cols, reply))
                    .await
            }
            Request::Cancel { id, generation } => self.cancel(id, generation).await,
        }
    }
    async fn ended(&self, id: &str, generation: &str) -> Result<bool> {
        let row = self
            .store
            .list()
            .await?
            .into_iter()
            .find(|s| s.launch.id == id)
            .context("session not found")?;
        anyhow::ensure!(row.generation == generation, "stale generation");
        Ok(matches!(
            row.status,
            Status::Exited | Status::Failed | Status::Interrupted
        ))
    }
    async fn cancel(&self, id: String, generation: String) -> Result<Value> {
        if self.ended(&id, &generation).await? {
            return Ok(json!({"accepted":true,"delivery":"already_ended"}));
        }
        match self
            .control(id.clone(), generation.clone(), Control::Cancel)
            .await
        {
            Ok(value) => Ok(value),
            Err(error) => {
                if self.ended(&id, &generation).await? {
                    Ok(json!({"accepted":true,"delivery":"already_ended"}))
                } else {
                    Err(error)
                }
            }
        }
    }
    async fn control(
        &self,
        id: String,
        generation: String,
        make: impl FnOnce(ControlReply) -> Control,
    ) -> Result<Value> {
        let (tx, rx) = oneshot::channel();
        {
            let sessions = self.sessions.lock().await;
            let live = sessions.get(&id).context("terminal not available")?;
            anyhow::ensure!(live.generation == generation, "stale generation");
            live.controls
                .try_send(make(tx))
                .map_err(|_| anyhow::anyhow!("terminal stopped or command queue full"))?;
        }
        tokio::time::timeout(Duration::from_secs(5), rx)
            .await
            .context("command outcome unknown; reconcile session state")??
    }
    async fn launch(&self, launch: Launch) -> Result<Value> {
        launch.validate()?;
        let _serial = self.launches.lock().await;
        anyhow::ensure!(!self.stopping.load(Ordering::SeqCst), "host is stopping");
        let records = self.store.list().await?;
        if let Some(existing) = records.iter().find(|s| s.launch.id == launch.id) {
            anyhow::ensure!(
                existing.launch == launch,
                "id already used for a different launch"
            );
            return Ok(serde_json::to_value(existing)?);
        }
        anyhow::ensure!(
            records.len() < 128,
            "prototype record limit reached; use a new state directory after stopping sessions"
        );
        anyhow::ensure!(
            records
                .iter()
                .filter(|s| matches!(
                    s.status,
                    Status::Starting | Status::Running | Status::Cancelling
                ))
                .count()
                < MAX_SESSIONS,
            "live session limit reached"
        );
        let session = self.store.create(launch.clone()).await?;
        let ring = Arc::new(Mutex::new(Ring::default()));
        let (control_tx, control_rx) = blocking::sync_channel(32);
        let (ready_tx, ready_rx) = oneshot::channel();
        let worker_session = session.clone();
        let worker_ring = ring.clone();
        let store = self.store.clone();
        let runtime = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            if let Err(e) = terminal(
                worker_session.clone(),
                worker_ring.clone(),
                control_rx,
                ready_tx,
                &store,
                &runtime,
            ) {
                worker_ring.lock().unwrap().eof = true;
                let _ = runtime.block_on(store.set(
                    &worker_session.launch.id,
                    &worker_session.generation,
                    Status::Failed,
                    None,
                    Some(e.to_string()),
                ));
            }
        });
        // Reserve before awaiting readiness: disconnected clients do not cancel launch effects.
        self.sessions.lock().await.insert(
            launch.id.clone(),
            Live {
                generation: session.generation.clone(),
                controls: control_tx,
                ring,
            },
        );
        drop(_serial);
        tokio::time::timeout(Duration::from_secs(4), ready_rx)
            .await
            .context("startup outcome unknown; reconcile using the same launch id")?
            .context("terminal startup failed; inspect session record")??;
        Ok(serde_json::to_value(
            self.store
                .list()
                .await?
                .into_iter()
                .find(|s| s.launch.id == launch.id)
                .context("missing session")?,
        )?)
    }
}

fn terminal(
    session: Session,
    ring: Arc<Mutex<Ring>>,
    controls: blocking::Receiver<Control>,
    ready: oneshot::Sender<Result<()>>,
    store: &Store,
    runtime: &tokio::runtime::Handle,
) -> Result<()> {
    let setup = (|| {
        let pair = native_pty_system().openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut command = CommandBuilder::new(&session.launch.program);
        command.args(&session.launch.args);
        command.cwd(&session.launch.cwd);
        command.env("TERM", "xterm-256color");
        // Clone descriptors before spawn so failed setup cannot strand a child.
        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);
        Ok::<_, anyhow::Error>((pair.master, reader, writer, child))
    })();
    let (master, mut reader, mut writer, mut child) = match setup {
        Ok(parts) => parts,
        Err(e) => {
            let message = e.to_string();
            let _ = ready.send(Err(anyhow::anyhow!(message)));
            return Err(e);
        }
    };
    let mut owned = OwnedChild {
        pid: child.process_id().map(|p| p as i32),
        child: child.as_mut(),
        reaped: false,
    };
    let pid = owned.pid.context("PTY child lacks process identity")?;
    let fd = master
        .as_raw_fd()
        .context("PTY lacks a pollable descriptor")?;
    // SAFETY: the descriptor belongs to master and remains open during this call.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    anyhow::ensure!(
        flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0,
        "cannot make PTY nonblocking"
    );
    // The polling thread owns a CLOEXEC duplicate, so dropping master cannot
    // leave poll observing a recycled descriptor number.
    let poll_fd = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    anyhow::ensure!(poll_fd >= 0, "cannot duplicate PTY polling descriptor");
    // SAFETY: fcntl returned a new owned descriptor and no other owner exists.
    let reader_poll = unsafe { OwnedFd::from_raw_fd(poll_fd) };
    if let Err(e) = runtime.block_on(store.set(
        &session.launch.id,
        &session.generation,
        Status::Running,
        None,
        None,
    )) {
        let _ = ready.send(Err(anyhow::anyhow!(e.to_string())));
        return Err(e);
    }
    let stop_io = Arc::new(AtomicBool::new(false));
    let _stop_io = StopIo(stop_io.clone());
    let output = ring.clone();
    let reader_stop = stop_io.clone();
    std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        loop {
            if reader_stop.load(Ordering::Relaxed) {
                break;
            }
            match reader.read(&mut bytes) {
                Ok(0) => break,
                Ok(n) => output.lock().unwrap().append(&bytes[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    let mut poll = libc::pollfd {
                        fd: reader_poll.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    // SAFETY: poll points to one valid entry and reader_poll
                    // owns the descriptor for this thread's entire lifetime.
                    let result = unsafe { libc::poll(&mut poll, 1, 100) };
                    if (result < 0
                        && std::io::Error::last_os_error().kind()
                            != std::io::ErrorKind::Interrupted)
                        || poll.revents & libc::POLLNVAL != 0
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        output.lock().unwrap().eof = true;
    });
    let (input_tx, input_rx) = blocking::sync_channel::<Vec<u8>>(8);
    let writer_stop = stop_io.clone();
    std::thread::spawn(move || {
        'writer: while !writer_stop.load(Ordering::Relaxed) {
            let bytes = match input_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(bytes) => bytes,
                Err(blocking::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            };
            let mut remaining = bytes.as_slice();
            while !remaining.is_empty() {
                if writer_stop.load(Ordering::Relaxed) {
                    break 'writer;
                }
                match writer.write(remaining) {
                    Ok(0) => break 'writer,
                    Ok(n) => remaining = &remaining[n..],
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => break 'writer,
                }
            }
        }
    });
    let _ = ready.send(Ok(()));
    loop {
        // Observe without reaping: the PID remains reserved while its original
        // process group is cleaned. Never signal an ID after wait reaps it.
        if owned.exited()? {
            let status = owned.finish()?;
            runtime.block_on(store.set(
                &session.launch.id,
                &session.generation,
                Status::Exited,
                Some(status.exit_code()),
                None,
            ))?;
            // Bound drain time even if a descendant escaped the original group.
            for _ in 0..100 {
                if ring.lock().unwrap().eof {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            break;
        }
        match controls.recv_timeout(Duration::from_millis(20)) {
            Ok(Control::Input(data, reply)) => {
                let result = input_tx
                    .try_send(data)
                    .map(|()| json!({"accepted":true,"delivery":"queued"}))
                    .map_err(|_| anyhow::anyhow!("input queue full or writer closed"));
                let _ = reply.send(result);
            }
            Ok(Control::Resize(rows, cols, reply)) => {
                let result = master
                    .resize(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    })
                    .map(|()| json!({"resized":true}));
                let _ = reply.send(result);
            }
            Ok(Control::Cancel(reply)) => {
                let result = (|| {
                    runtime.block_on(store.set(
                        &session.launch.id,
                        &session.generation,
                        Status::Cancelling,
                        None,
                        None,
                    ))?;
                    kill_group(pid)?;
                    Ok(json!({"accepted":true,"delivery":"signal_sent"}))
                })();
                let _ = reply.send(result);
            }
            Err(blocking::RecvTimeoutError::Disconnected) => {
                let status = owned.finish()?;
                runtime.block_on(store.set(
                    &session.launch.id,
                    &session.generation,
                    Status::Exited,
                    Some(status.exit_code()),
                    None,
                ))?;
                break;
            }
            Err(blocking::RecvTimeoutError::Timeout) => {}
        }
    }
    Ok(())
}
struct StopIo(Arc<AtomicBool>);
impl Drop for StopIo {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

// Exclusively owns reaping. The guard handles every post-spawn early return.
struct OwnedChild<'a> {
    child: &'a mut dyn portable_pty::Child,
    pid: Option<i32>,
    reaped: bool,
}
impl OwnedChild<'_> {
    #[cfg(target_os = "linux")]
    fn exited(&mut self) -> Result<bool> {
        let pid = self.pid.context("missing child PID")?;
        // SAFETY: zeroed siginfo is valid output storage; P_PID names our child.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } != 0
        {
            let error = std::io::Error::last_os_error();
            // If ownership was lost, this number must never be signalled.
            if error.raw_os_error() == Some(libc::ECHILD) {
                self.reaped = true;
            }
            return Err(error.into());
        }
        Ok(unsafe { info.si_pid() } != 0)
    }
    #[cfg(not(target_os = "linux"))]
    fn exited(&mut self) -> Result<bool> {
        bail!("native session host currently requires Linux")
    }
    fn finish(&mut self) -> Result<portable_pty::ExitStatus> {
        anyhow::ensure!(!self.reaped, "child already reaped");
        if let Some(pid) = self.pid {
            kill_group(pid)?;
        } else {
            self.child.kill()?;
        }
        // Disarm before wait: even an error must not cause a later stale signal.
        self.reaped = true;
        Ok(self.child.wait()?)
    }
}
impl Drop for OwnedChild<'_> {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.finish();
        }
    }
}

fn kill_group(pid: i32) -> Result<()> {
    anyhow::ensure!(pid > 1, "invalid process group");
    // SAFETY: signal targets the group of a child still owned and unreaped by
    // this worker. portable-pty calls setsid before exec on Unix.
    if unsafe { libc::kill(-pid, libc::SIGKILL) } != 0 {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::ESRCH) {
            return Err(e.into());
        }
    }
    Ok(())
}

pub async fn read_frame(stream: &mut UnixStream) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut reader = BufReader::new(stream.take((FRAME_LIMIT + 1) as u64));
    tokio::time::timeout(Duration::from_secs(5), reader.read_until(b'\n', &mut bytes)).await??;
    anyhow::ensure!(
        bytes.len() <= FRAME_LIMIT && bytes.last() == Some(&b'\n'),
        "invalid or oversized frame"
    );
    Ok(bytes)
}
pub async fn request(dir: &Path, request: &Request) -> Result<Reply> {
    let mut stream = UnixStream::connect(dir.join("host.sock")).await?;
    let mut bytes = serde_json::to_vec(request)?;
    bytes.push(b'\n');
    anyhow::ensure!(bytes.len() <= FRAME_LIMIT, "request frame exceeds limit");
    tokio::time::timeout(Duration::from_secs(5), stream.write_all(&bytes)).await??;
    Ok(serde_json::from_slice(&read_frame(&mut stream).await?)?)
}
pub async fn run(dir: &Path) -> Result<()> {
    anyhow::ensure!(
        cfg!(target_os = "linux"),
        "native session host currently requires Linux"
    );
    let (listener, _guard) = bind_socket(dir, "host")?;
    let host = Arc::new(Host {
        store: Store::open(&dir.join("state.db")).await?,
        sessions: AsyncMutex::new(HashMap::new()),
        launches: AsyncMutex::new(()),
        stopping: AtomicBool::new(false),
    });
    let permits = Arc::new(Semaphore::new(64));
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        tokio::select! {
            _=tokio::signal::ctrl_c()=>break,
            _=terminate.recv()=>break,
            connection=listener.accept()=> {
                let (mut stream,_)=connection?;
                let Ok(permit)=permits.clone().try_acquire_owned() else {continue};
                let host=host.clone();
                tokio::spawn(async move {
                    let _permit=permit;
                    let result=async {
                        let request=serde_json::from_slice(&read_frame(&mut stream).await?)?;
                        host.dispatch(request).await
                    }.await;
                    let reply=Reply::from_result(result);
                    if let Ok(mut bytes)=serde_json::to_vec(&reply) {
                        bytes.push(b'\n');
                        if bytes.len()>FRAME_LIMIT {
                            bytes=b"{\"ok\":false,\"error\":\"response exceeds frame limit\"}\n".to_vec();
                        }
                        let _=tokio::time::timeout(Duration::from_secs(5),stream.write_all(&bytes)).await;
                    }
                });
            }
        }
    }
    // Fence admission and wait until every admitted worker is registered.
    {
        let _launches = host.launches.lock().await;
        host.stopping.store(true, Ordering::SeqCst);
    }
    // A host stop deliberately ends its sessions. API shutdown never enters here.
    let sessions = host.store.list().await?;
    for session in sessions {
        if matches!(
            session.status,
            Status::Starting | Status::Running | Status::Cancelling
        ) {
            let _ = host
                .dispatch(Request::Cancel {
                    id: session.launch.id,
                    generation: session.generation,
                })
                .await;
        }
    }
    for _ in 0..100 {
        if !host.store.list().await?.iter().any(|s| {
            matches!(
                s.status,
                Status::Starting | Status::Running | Status::Cancelling
            )
        }) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    bail!("host shutdown could not confirm every terminal exit")
}
