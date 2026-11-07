//! Platform smoke test, not proof of Windows process-tree cleanup.
//! Keep ConPTY output draining during close and answer its inherited-cursor query.
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

fn main() -> anyhow::Result<()> {
    // Covers OS startup/teardown calls as well as reads. A probe must fail boundedly.
    let finished = Arc::new(AtomicBool::new(false));
    let watchdog = finished.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        if !watchdog.load(Ordering::SeqCst) {
            eprintln!("PTY probe exceeded 30-second deadline");
            std::process::exit(124);
        }
    });
    eprintln!("probe: opening PTY");
    let pair = native_pty_system().openpty(PtySize::default())?;
    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;
    let (input_tx, input_rx) = mpsc::sync_channel::<Vec<u8>>(8);
    let input_worker = std::thread::spawn(move || -> std::io::Result<()> {
        while let Ok(bytes) = input_rx.recv() {
            writer.write_all(&bytes)?;
            writer.flush()?;
        }
        Ok(())
    });
    let output_worker = std::thread::spawn(move || -> anyhow::Result<(bool, usize)> {
        let mut bytes = [0; 4096];
        let mut marker_tail = Vec::new();
        let mut query_tail = Vec::new();
        let mut marker_seen = false;
        let mut queries = 0;
        loop {
            let n = match reader.read(&mut bytes) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => break,
                Err(e) => return Err(e.into()),
            };
            marker_tail.extend_from_slice(&bytes[..n]);
            marker_seen |= marker_tail
                .windows(b"TANGLECLAW_PTY_OK".len())
                .any(|w| w == b"TANGLECLAW_PTY_OK");
            let retain = marker_tail.len().saturating_sub(32);
            marker_tail.drain(..retain);
            query_tail.extend_from_slice(&bytes[..n]);
            for _ in 0..query_tail.windows(4).filter(|w| *w == b"\x1b[6n").count() {
                queries += 1;
                anyhow::ensure!(queries <= 16, "unexpected cursor-query flood");
                input_tx.try_send(b"\x1b[1;1R".to_vec())?;
                eprintln!("probe: answered inherited-cursor query");
            }
            let retain = query_tail.len().saturating_sub(3);
            query_tail.drain(..retain);
        }
        Ok((marker_seen, queries))
    });
    #[cfg(windows)]
    let mut cmd = CommandBuilder::new("cmd.exe");
    #[cfg(windows)]
    cmd.args(["/c", "echo TANGLECLAW_PTY_OK"]);
    #[cfg(not(windows))]
    let mut cmd = CommandBuilder::new("/bin/sh");
    #[cfg(not(windows))]
    cmd.args(["-c", "printf 'TANGLECLAW_PTY_OK\\n'"]);
    let mut child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);
    eprintln!("probe: child spawned");
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            anyhow::bail!("PTY child exceeded exit deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    eprintln!("probe: child exited; closing PTY while draining output");
    // ConPTY may emit a final frame while ClosePseudoConsole blocks. The output
    // thread must still be reading here, including servicing cursor queries.
    drop(pair.master);
    let (marker_seen, queries) = output_worker
        .join()
        .map_err(|_| anyhow::anyhow!("output worker panicked"))??;
    let _ = input_worker
        .join()
        .map_err(|_| anyhow::anyhow!("input worker panicked"))?;
    anyhow::ensure!(status.success(), "PTY child failed");
    anyhow::ensure!(marker_seen, "PTY marker missing");
    finished.store(true, Ordering::SeqCst);
    println!("PTY spawn/output/exit/teardown passed; cursor_queries={queries}");
    Ok(())
}
