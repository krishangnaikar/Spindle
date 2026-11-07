use clap::{Parser, Subcommand};
use std::path::PathBuf;
#[derive(Parser)]
#[command(
    name = "spindle",
    version,
    about = "Persistent local sessions for coding agents"
)]
struct Cli {
    #[arg(long, global = true, default_value = ".state")]
    state_dir: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Own terminals until explicitly stopped. Run separately from the API.
    Host,
    /// Serve HTTP on a private Unix socket; restart without losing terminals.
    Api,
    Health,
    List,
    /// Launch exactly once per ID; identical retries return the existing record.
    Launch {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        cwd: Option<PathBuf>,
        program: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Read a bounded page of raw terminal bytes encoded as JSON integers.
    Output {
        id: String,
        generation: String,
        #[arg(long, default_value_t = 0)]
        after: u64,
    },
    /// Queue terminal input. Append newline explicitly when submitting a line.
    Input {
        id: String,
        generation: String,
        data: String,
    },
    Resize {
        id: String,
        generation: String,
        rows: u16,
        cols: u16,
    },
    /// Force-stop the session's original Unix process group.
    Cancel {
        id: String,
        generation: String,
    },
}
#[cfg(unix)]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use spindle::{
        host,
        model::{Launch, Request},
    };
    let cli = Cli::parse();
    let request = match cli.command {
        Command::Host => return host::run(&cli.state_dir).await,
        Command::Api => return spindle::api::run(&cli.state_dir).await,
        Command::Health => Request::Health,
        Command::List => Request::List,
        Command::Launch {
            id,
            cwd,
            program,
            args,
        } => Request::Launch {
            launch: Launch {
                id: id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                program,
                args,
                cwd: cwd
                    .unwrap_or(std::env::current_dir()?)
                    .canonicalize()?
                    .to_string_lossy()
                    .into_owned(),
            },
        },
        Command::Output {
            id,
            generation,
            after,
        } => Request::Output {
            id,
            generation,
            after,
        },
        Command::Input {
            id,
            generation,
            data,
        } => Request::Input {
            id,
            generation,
            data: data.into_bytes(),
        },
        Command::Resize {
            id,
            generation,
            rows,
            cols,
        } => Request::Resize {
            id,
            generation,
            rows,
            cols,
        },
        Command::Cancel { id, generation } => Request::Cancel { id, generation },
    };
    let reply = host::request(&cli.state_dir, &request).await?;
    println!("{}", serde_json::to_string_pretty(&reply)?);
    anyhow::ensure!(reply.ok, "command failed");
    Ok(())
}
#[cfg(not(unix))]
fn main() -> anyhow::Result<()> {
    let _ = Cli::parse();
    anyhow::bail!(
        "session host IPC is Unix-only in this milestone; run the pty_probe example for the native PTY spike"
    )
}
