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

#[derive(Default)]
struct ProbeOutput {
    marker_tail: Vec<u8>,
    query_tail: Vec<u8>,
    marker_seen: bool,
    queries: usize,
}
impl ProbeOutput {
    fn feed(&mut self, bytes: &[u8]) -> anyhow::Result<usize> {
        self.marker_tail.extend_from_slice(bytes);
        self.marker_seen |= self
            .marker_tail
            .windows(b"TANGLECLAW_PTY_OK".len())
            .any(|w| w == b"TANGLECLAW_PTY_OK");
        let retain = self.marker_tail.len().saturating_sub(32);
        self.marker_tail.drain(..retain);
        self.query_tail.extend_from_slice(bytes);
        let found = self
            .query_tail
            .windows(4)
            .filter(|w| *w == b"\x1b[6n")
            .count();
        anyhow::ensure!(self.queries + found <= 16, "unexpected cursor-query flood");
        self.queries += found;
        let retain = self.query_tail.len().saturating_sub(3);
        self.query_tail.drain(..retain);
        Ok(found)
    }
}
fn drain_output(
    mut reader: impl Read,
    input: &mpsc::SyncSender<Vec<u8>>,
) -> anyhow::Result<(bool, usize)> {
    let mut output = ProbeOutput::default();
    let mut bytes = [0; 4096];
    loop {
        let n = match reader.read(&mut bytes) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => break,
            Err(e) => return Err(e.into()),
        };
        for _ in 0..output.feed(&bytes[..n])? {
            input.try_send(b"\x1b[1;1R".to_vec())?;
            eprintln!("probe: answered inherited-cursor query");
        }
    }
    Ok((output.marker_seen, output.queries))
}

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
    let reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;
    let (input_tx, input_rx) = mpsc::sync_channel::<Vec<u8>>(8);
    let input_worker = std::thread::spawn(move || -> std::io::Result<()> {
        while let Ok(bytes) = input_rx.recv() {
            writer.write_all(&bytes)?;
            writer.flush()?;
        }
        Ok(())
    });
    let output_worker = std::thread::spawn(move || drain_output(reader, &input_tx));
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_query_recognized_once_at_every_split_boundary() {
        for split in 0..=4 {
            let mut output = ProbeOutput::default();
            let query = b"\x1b[6n";
            assert_eq!(
                output.feed(&query[..split]).unwrap() + output.feed(&query[split..]).unwrap(),
                1
            );
            assert_eq!(output.feed(b"tail").unwrap(), 0);
            assert_eq!(output.queries, 1);
        }
    }
    #[test]
    fn marker_recognized_at_every_split_and_remains_observed() {
        let marker = b"TANGLECLAW_PTY_OK";
        for split in 0..=marker.len() {
            let mut output = ProbeOutput::default();
            output.feed(&marker[..split]).unwrap();
            output.feed(&marker[split..]).unwrap();
            assert!(output.marker_seen);
            output.feed(&[b'x'; 4096]).unwrap();
            assert!(output.marker_seen);
        }
    }
    #[test]
    fn unrelated_sequences_and_empty_chunks_do_not_generate_input() {
        let mut output = ProbeOutput::default();
        for bytes in [
            b"".as_slice(),
            b"[6n",
            b"\x1b[5n",
            b"\x1b[6R",
            b"TANGLECLAW_PTY_O",
        ] {
            assert_eq!(output.feed(bytes).unwrap(), 0);
        }
        assert!(!output.marker_seen);
    }
    #[test]
    fn query_limit_is_inclusive_and_parser_retention_is_bounded() {
        let mut output = ProbeOutput::default();
        for _ in 0..16 {
            assert_eq!(output.feed(b"\x1b[6n").unwrap(), 1);
        }
        assert!(output.feed(b"\x1b[6n").is_err());
        let mut output = ProbeOutput::default();
        for _ in 0..1000 {
            output.feed(&[255; 4096]).unwrap();
            assert!(output.marker_tail.len() <= 32 && output.query_tail.len() <= 3);
        }
    }
    #[test]
    fn drain_sends_exact_reply_and_tracks_multiple_queries() {
        let (tx, rx) = mpsc::sync_channel(8);
        let result = drain_output(
            std::io::Cursor::new(b"\x1b[6n\x1b[6nTANGLECLAW_PTY_OK"),
            &tx,
        )
        .unwrap();
        assert_eq!(result, (true, 2));
        assert_eq!(rx.try_recv().unwrap(), b"\x1b[1;1R");
        assert_eq!(rx.try_recv().unwrap(), b"\x1b[1;1R");
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn drain_fails_when_reply_queue_is_full_or_closed() {
        let (tx, _rx) = mpsc::sync_channel(0);
        assert!(drain_output(std::io::Cursor::new(b"\x1b[6n"), &tx).is_err());
        let (tx, rx) = mpsc::sync_channel(8);
        drop(rx);
        assert!(drain_output(std::io::Cursor::new(b"\x1b[6n"), &tx).is_err());
    }
    struct FaultReader {
        kinds: std::collections::VecDeque<std::io::ErrorKind>,
    }
    impl Read for FaultReader {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            match self.kinds.pop_front() {
                Some(kind) => Err(std::io::Error::from(kind)),
                None => Ok(0),
            }
        }
    }
    #[test]
    fn drain_retries_interruptions_accepts_broken_pipe_and_propagates_errors() {
        use std::io::ErrorKind::*;
        let (tx, _rx) = mpsc::sync_channel(8);
        assert_eq!(
            drain_output(
                FaultReader {
                    kinds: [Interrupted, BrokenPipe].into()
                },
                &tx
            )
            .unwrap(),
            (false, 0)
        );
        assert!(
            drain_output(
                FaultReader {
                    kinds: [PermissionDenied].into()
                },
                &tx
            )
            .is_err()
        );
    }
}
