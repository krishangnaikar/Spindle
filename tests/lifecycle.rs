#![cfg(target_os = "linux")]
use spindle::{
    host,
    model::{FRAME_LIMIT, Launch, Output, Reply, Request, Session},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

struct Process(Child);
impl Process {
    fn spawn(dir: &Path, mode: &str) -> Self {
        Self(
            Command::new(env!("CARGO_BIN_EXE_spindle"))
                .arg("--state-dir")
                .arg(dir)
                .arg(mode)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.kill();
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    state: PathBuf,
    host: Process,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        let host = Process::spawn(&state, "host");
        let f = Self {
            _dir: dir,
            state,
            host,
        };
        f.ready().await;
        f
    }
    async fn ready(&self) {
        let start = Instant::now();
        loop {
            if let Ok(reply) = host::request(&self.state, &Request::Health).await
                && reply.ok
            {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "host did not start"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    fn launch_spec(&self, script: &str) -> Launch {
        Launch {
            id: uuid::Uuid::new_v4().to_string(),
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: self._dir.path().to_str().unwrap().into(),
        }
    }
    async fn launch(&self, script: &str) -> Session {
        let reply = self
            .call(Request::Launch {
                launch: self.launch_spec(script),
            })
            .await;
        serde_json::from_value(reply.value.unwrap()).unwrap()
    }
    async fn call(&self, request: Request) -> Reply {
        let reply = host::request(&self.state, &request).await.unwrap();
        assert!(reply.ok, "{:?}", reply.error);
        reply
    }
    async fn output(&self, session: &Session, after: u64) -> Output {
        serde_json::from_value(
            self.call(Request::Output {
                id: session.launch.id.clone(),
                generation: session.generation.clone(),
                after,
            })
            .await
            .value
            .unwrap(),
        )
        .unwrap()
    }
    async fn contains(&self, session: &Session, needle: &str) -> String {
        let start = Instant::now();
        let mut after = 0;
        let mut bytes = Vec::new();
        loop {
            let page = self.output(session, after).await;
            after = page.next;
            bytes.extend(page.bytes);
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if text.contains(needle) {
                return text;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "missing output {needle}: {text}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    async fn terminal_status(&self, id: &str) -> String {
        let start = Instant::now();
        loop {
            let values = self.call(Request::List).await.value.unwrap();
            let row = values
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["id"] == id)
                .unwrap();
            let status = row["status"].as_str().unwrap();
            if matches!(status, "exited" | "failed" | "interrupted") {
                return status.into();
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "terminal did not stop"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    async fn cancel(&self, session: &Session) {
        self.call(Request::Cancel {
            id: session.launch.id.clone(),
            generation: session.generation.clone(),
        })
        .await;
        assert_eq!(self.terminal_status(&session.launch.id).await, "exited");
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Graceful host shutdown owns cleanup of all test sessions, then bounded
        // kill fallback. This never targets a PID recovered from persistent state.
        unsafe {
            libc::kill(self.host.0.id() as i32, libc::SIGTERM);
        }
        let end = Instant::now() + Duration::from_secs(3);
        while Instant::now() < end {
            if matches!(self.host.0.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.host.kill();
    }
}
async fn api_ready(dir: &Path) {
    for _ in 0..100 {
        if UnixStream::connect(dir.join("api.sock")).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("API did not start")
}
async fn api_command(dir: &Path, request: &Request) -> Reply {
    let body = serde_json::to_vec(request).unwrap();
    let mut socket = UnixStream::connect(dir.join("api.sock")).await.unwrap();
    let header = format!(
        "POST /v1/command HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(header.as_bytes()).await.unwrap();
    socket.write_all(&body).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), socket.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    let split = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    assert!(response.starts_with(b"HTTP/1.1 200"));
    serde_json::from_slice(&response[split..]).unwrap()
}

#[tokio::test]
async fn terminal_survives_api_restart_input_resize_and_reconnect() {
    let f = Fixture::new().await;
    let mut api = Process::spawn(&f.state, "api");
    api_ready(&f.state).await;
    let launch=f.launch_spec("stty -echo; printf READY; while IFS= read -r line; do printf '\nREPLY:%s\n' \"$line\"; stty size; done");
    let reply = api_command(&f.state, &Request::Launch { launch }).await;
    assert!(reply.ok, "{:?}", reply.error);
    let session: Session = serde_json::from_value(reply.value.unwrap()).unwrap();
    f.contains(&session, "READY").await;
    let cursor = f.output(&session, 0).await.next;
    api.kill();
    f.call(Request::Resize {
        id: session.launch.id.clone(),
        generation: session.generation.clone(),
        rows: 42,
        cols: 100,
    })
    .await;
    f.call(Request::Input {
        id: session.launch.id.clone(),
        generation: session.generation.clone(),
        data: b"after-restart\n".to_vec(),
    })
    .await;
    let _api = Process::spawn(&f.state, "api");
    api_ready(&f.state).await;
    let reply = api_command(
        &f.state,
        &Request::Output {
            id: session.launch.id.clone(),
            generation: session.generation.clone(),
            after: cursor,
        },
    )
    .await;
    assert!(reply.ok);
    f.contains(&session, "REPLY:after-restart").await;
    f.contains(&session, "42 100").await;
    f.cancel(&session).await;
}

#[tokio::test]
async fn concurrent_duplicate_launch_is_once_and_conflicts_are_rejected() {
    let f = Fixture::new().await;
    let launch = f.launch_spec("printf x >> starts; sleep 120");
    let request = Request::Launch {
        launch: launch.clone(),
    };
    let (a, b) = tokio::join!(
        host::request(&f.state, &request),
        host::request(&f.state, &request)
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert!(a.ok && b.ok);
    let session: Session = serde_json::from_value(a.value.unwrap()).unwrap();
    assert_eq!(b.value.unwrap()["generation"], session.generation);
    for _ in 0..100 {
        if fs::read(f._dir.path().join("starts")).is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(fs::read(f._dir.path().join("starts")).unwrap(), b"x");
    let mut conflict = launch;
    conflict.args = vec!["-c".into(), "exit 0".into()];
    assert!(
        !host::request(&f.state, &Request::Launch { launch: conflict })
            .await
            .unwrap()
            .ok
    );
    assert!(
        !host::request(
            &f.state,
            &Request::Cancel {
                id: session.launch.id.clone(),
                generation: uuid::Uuid::new_v4().to_string()
            }
        )
        .await
        .unwrap()
        .ok
    );
    f.cancel(&session).await;
    // A retried stop cannot become a new effect or an error after completion.
    f.call(Request::Cancel {
        id: session.launch.id.clone(),
        generation: session.generation.clone(),
    })
    .await;
}

#[tokio::test]
async fn output_flood_is_bounded_and_reports_replay_gap() {
    let f = Fixture::new().await;
    let session = f
        .launch("head -c 400000 /dev/zero; printf END; sleep 120")
        .await;
    let start = Instant::now();
    loop {
        let output = f.output(&session, 0).await;
        if output.gap {
            assert!(output.bytes.len() <= 8192);
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    f.call(Request::Health).await;
    f.cancel(&session).await;
}

#[tokio::test]
async fn failed_executable_is_durable_and_host_restart_marks_interrupted() {
    let mut f = Fixture::new().await;
    let mut launch = f.launch_spec("exit 0");
    launch.program = "/definitely/not/a/program".into();
    let id = launch.id.clone();
    assert!(
        !host::request(&f.state, &Request::Launch { launch })
            .await
            .unwrap()
            .ok
    );
    assert_eq!(f.terminal_status(&id).await, "failed");
    let session = f.launch("printf LIVE; sleep 2").await;
    f.contains(&session, "LIVE").await;
    f.host.kill();
    f.host = Process::spawn(&f.state, "host");
    f.ready().await;
    assert_eq!(f.terminal_status(&session.launch.id).await, "interrupted");
    let replay = f
        .call(Request::Launch {
            launch: session.launch.clone(),
        })
        .await;
    assert_eq!(replay.value.unwrap()["status"], "interrupted");
    let output = host::request(
        &f.state,
        &Request::Output {
            id: session.launch.id,
            generation: session.generation,
            after: 0,
        },
    )
    .await
    .unwrap();
    assert!(
        !output.ok,
        "host restart cannot pretend to recover in-memory output"
    );
}
fn running(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| s.rsplit_once(") ").map(|(_, v)| !v.starts_with('Z')))
        .unwrap_or(false)
}
#[tokio::test]
async fn cancel_and_natural_leader_exit_clean_original_process_group() {
    let f = Fixture::new().await;
    for script in [
        "sleep 120 & printf 'CHILD:%s\n' \"$!\"; wait",
        "sleep 120 & printf 'CHILD:%s\n' \"$!\"; sleep 0.2; exit 0",
    ] {
        let session = f.launch(script).await;
        let text = f.contains(&session, "CHILD:").await;
        let pid: u32 = text
            .split("CHILD:")
            .nth(1)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        if script.ends_with("wait") {
            f.cancel(&session).await;
        } else {
            assert_eq!(f.terminal_status(&session.launch.id).await, "exited");
        }
        for _ in 0..100 {
            if !running(pid) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!running(pid), "original-group descendant still running");
    }
}
#[tokio::test]
async fn ipc_rejects_oversized_frames_and_second_host() {
    let f = Fixture::new().await;
    let mut stream = UnixStream::connect(f.state.join("host.sock"))
        .await
        .unwrap();
    stream
        .write_all(&vec![b'x'; FRAME_LIMIT + 1])
        .await
        .unwrap();
    let reply: Reply =
        serde_json::from_slice(&host::read_frame(&mut stream).await.unwrap()).unwrap();
    assert!(!reply.ok);
    let mut second = Process::spawn(&f.state, "host");
    let start = Instant::now();
    loop {
        if let Some(status) = second.0.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    f.call(Request::Health).await;
    let mode = fs::metadata(f.state.join("host.sock"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}
#[test]
fn ipc_refuses_public_directory_and_symlinked_database() {
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(host::private_dir(dir.path()).is_err());
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::symlink("/tmp/nonexistent-spindle-test", dir.path().join("state.db"))
        .unwrap();
    assert!(host::private_dir(dir.path()).is_err());
}
#[tokio::test]
async fn list_stays_bounded_with_large_launch_arguments() {
    let f = Fixture::new().await;
    let mut sessions = Vec::new();
    for _ in 0..4 {
        let mut launch = f.launch_spec("sleep 120");
        launch.args.extend(["padding".into(), "x".repeat(20000)]);
        let reply = f.call(Request::Launch { launch }).await;
        sessions.push(serde_json::from_value::<Session>(reply.value.unwrap()).unwrap());
    }
    let reply = f.call(Request::List).await;
    assert!(serde_json::to_vec(&reply).unwrap().len() < FRAME_LIMIT);
    assert_eq!(reply.value.unwrap().as_array().unwrap().len(), 4);
    for session in sessions {
        f.cancel(&session).await;
    }
}

#[tokio::test]
async fn graceful_shutdown_fences_concurrent_launches_and_reaps_admitted_groups() {
    let mut f = Fixture::new().await;
    let session = f
        .launch("printf '%s' \"$$\" > child-initial; sleep 120")
        .await;
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let id = uuid::Uuid::new_v4().to_string();
        let launch = f.launch_spec(&format!("printf '%s' \"$$\" > child-{id}; sleep 120"));
        let dir = f.state.clone();
        requests.spawn(async move { host::request(&dir, &Request::Launch { launch }).await });
    }
    tokio::time::sleep(Duration::from_millis(5)).await;
    // The handle is a live test child, not an identity recovered from disk.
    unsafe {
        libc::kill(f.host.0.id() as i32, libc::SIGTERM);
    }
    let start = Instant::now();
    loop {
        if let Some(status) = f.host.0.try_wait().unwrap() {
            assert!(status.success(), "host could not confirm graceful shutdown");
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    while requests.join_next().await.is_some() {}
    let db = rusqlite::Connection::open(f.state.join("state.db")).unwrap();
    let mut stmt = db.prepare("SELECT value FROM sessions").unwrap();
    let values = stmt.query_map([], |row| row.get::<_, String>(0)).unwrap();
    let mut found_initial = false;
    for value in values {
        let record: Session = serde_json::from_str(&value.unwrap()).unwrap();
        found_initial |= record.launch.id == session.launch.id;
        assert!(matches!(
            record.status,
            spindle::model::Status::Exited | spindle::model::Status::Failed
        ));
    }
    assert!(found_initial);
    for entry in fs::read_dir(f._dir.path()).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name().to_string_lossy().starts_with("child-") {
            let pid: u32 = fs::read_to_string(entry.path()).unwrap().parse().unwrap();
            assert!(
                !running(pid),
                "admitted process still running after shutdown"
            );
        }
    }
}

#[tokio::test]
async fn gateway_bounds_idle_connections_before_parsing_bodies() {
    let f = Fixture::new().await;
    let _api = Process::spawn(&f.state, "api");
    api_ready(&f.state).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut held = Vec::new();
    for _ in 0..64 {
        held.push(UnixStream::connect(f.state.join("api.sock")).await.unwrap());
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut overflow = UnixStream::connect(f.state.join("api.sock")).await.unwrap();
    let mut byte = [0];
    let closed = tokio::time::timeout(Duration::from_secs(2), overflow.read(&mut byte)).await;
    assert!(
        matches!(closed, Ok(Ok(0)) | Ok(Err(_))),
        "excess connection was retained"
    );
    drop(held);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(api_command(&f.state, &Request::Health).await.ok);
    let mut slow = UnixStream::connect(f.state.join("api.sock")).await.unwrap();
    slow.write_all(b"POST /v1/command HTTP/1.1\r\nHost:")
        .await
        .unwrap();
    let mut response = Vec::new();
    assert!(
        tokio::time::timeout(Duration::from_secs(12), slow.read_to_end(&mut response))
            .await
            .is_ok(),
        "slow header escaped absolute connection deadline"
    );
}
