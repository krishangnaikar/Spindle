//! Platform smoke test, not proof of Windows process-tree cleanup.
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::Read;
fn main() -> anyhow::Result<()> {
    let pair = native_pty_system().openpty(PtySize::default())?;
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
    let mut reader = pair.master.try_clone_reader()?;
    let mut bytes = [0; 4096];
    let mut output = Vec::new();
    loop {
        let n = reader.read(&mut bytes)?;
        anyhow::ensure!(n > 0, "PTY closed before marker");
        output.extend_from_slice(&bytes[..n]);
        if String::from_utf8_lossy(&output).contains("TANGLECLAW_PTY_OK") {
            break;
        }
        anyhow::ensure!(output.len() < 8192, "PTY probe output limit exceeded");
    }
    anyhow::ensure!(child.wait()?.success());
    println!("PTY spawn/output/exit passed");
    Ok(())
}
