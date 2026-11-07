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
    /// Launch an installed CLI using its own login, configuration and permissions.
    Agent {
        #[arg(value_enum)]
        engine: spindle::engine::NativeEngine,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        model: Option<String>,
    },
    /// Send one bounded API request. Read the prompt from a UTF-8 file.
    Ask {
        #[arg(long)]
        provider: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        prompt_file: PathBuf,
        #[arg(long, default_value_t = 1024)]
        max_output_tokens: u32,
        /// Explicitly permit paid routes; otherwise only OpenRouter free IDs work.
        #[arg(long)]
        allow_paid: bool,
        /// Name of an environment variable, never the key value.
        #[arg(long)]
        api_key_env: Option<String>,
    },
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
#[cfg(any(unix, test))]
impl Command {
    fn into_request(self) -> anyhow::Result<spindle::model::Request> {
        use spindle::model::{Launch, Request};
        Ok(match self {
            Command::Host | Command::Api | Command::Ask { .. } => {
                anyhow::bail!("service commands do not produce protocol requests")
            }
            Command::Agent {
                engine,
                id,
                cwd,
                model,
            } => {
                let cwd = cwd.map_or_else(std::env::current_dir, Ok)?;
                Request::Launch {
                    launch: spindle::engine::prepare_launch(engine, id, cwd, model)?,
                }
            }
            Command::Health => Request::Health {},
            Command::List => Request::List {},
            Command::Launch {
                id,
                cwd,
                program,
                args,
            } => {
                let cwd = match cwd {
                    Some(cwd) => cwd,
                    None => std::env::current_dir()?,
                };
                let launch = Launch {
                    id: id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    program,
                    args,
                    cwd: cwd
                        .canonicalize()?
                        .into_os_string()
                        .into_string()
                        .map_err(|_| anyhow::anyhow!("working directory must be valid UTF-8"))?,
                };
                launch.validate()?;
                Request::Launch { launch }
            }
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
        })
    }
}
/// Bound prompt input before loading credentials or opening a connection.
fn read_prompt(path: &std::path::Path) -> anyhow::Result<String> {
    use std::io::Read;
    anyhow::ensure!(
        std::fs::metadata(path)?.is_file(),
        "prompt must be a regular file"
    );
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Refuse a replaced FIFO without blocking between path and fd checks.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    anyhow::ensure!(file.metadata()?.is_file(), "prompt must be a regular file");
    let mut bytes = Vec::new();
    file.take(65537).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 65536, "prompt exceeds 64 KiB");
    String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("prompt must be UTF-8"))
}

fn prepare_ask(
    provider: &str,
    model: String,
    prompt: String,
    max_output_tokens: u32,
    allow_paid: bool,
    api_key_env: Option<String>,
) -> anyhow::Result<(spindle::provider::CompletionRequest, String)> {
    use spindle::provider::{CompletionRequest, Provider};
    let provider: Provider = provider.parse()?;
    anyhow::ensure!(
        allow_paid || provider == Provider::Openrouter,
        "this route requires --allow-paid; use openrouter/free for free inference"
    );
    let name = api_key_env.unwrap_or_else(|| {
        match provider {
            Provider::Openai => "OPENAI_API_KEY",
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::Openrouter => "OPENROUTER_API_KEY",
        }
        .into()
    });
    anyhow::ensure!(
        name.len() <= 128
            && !name.is_empty()
            && name.bytes().enumerate().all(|(i, b)| b == b'_'
                || b.is_ascii_alphabetic()
                || (i > 0 && b.is_ascii_digit())),
        "invalid API key environment variable name"
    );
    let request = CompletionRequest {
        provider,
        model,
        prompt,
        max_output_tokens,
        free_only: !allow_paid,
    };
    request.validate()?;
    Ok((request, name))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Command::Ask {
        provider,
        model,
        prompt_file,
        max_output_tokens,
        allow_paid,
        api_key_env,
    } = cli.command
    {
        let (request, name) = prepare_ask(
            &provider,
            model,
            read_prompt(&prompt_file)?,
            max_output_tokens,
            allow_paid,
            api_key_env,
        )?;
        // Only the explicitly selected variable is read at invocation. Never
        // inspect CLI credential stores or include credentials in launch records.
        let key = std::env::var(&name).map_err(|_| {
            anyhow::anyhow!("selected API key environment variable is missing or invalid")
        })?;
        let completion = spindle::provider::complete(&request, &key).await?;
        println!("{}", serde_json::to_string_pretty(&completion)?);
        return Ok(());
    }
    #[cfg(unix)]
    {
        let request = match cli.command {
            Command::Host => return spindle::host::run(&cli.state_dir).await,
            Command::Api => return spindle::api::run(&cli.state_dir).await,
            command => command.into_request()?,
        };
        let reply = spindle::host::request(&cli.state_dir, &request).await?;
        println!("{}", serde_json::to_string_pretty(&reply)?);
        anyhow::ensure!(reply.ok, "command failed");
        Ok(())
    }
    #[cfg(not(unix))]
    anyhow::bail!(
        "session host IPC is Unix-only in this milestone; API requests are supported, or run the pty_probe example for the native PTY spike"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use spindle::model::Request;
    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("spindle").chain(args.iter().copied())).unwrap()
    }
    #[test]
    fn cli_definition_is_consistent_and_help_version_succeed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
        assert_eq!(
            Cli::try_parse_from(["spindle", "--help"])
                .err()
                .unwrap()
                .kind(),
            clap::error::ErrorKind::DisplayHelp
        );
        assert_eq!(
            Cli::try_parse_from(["spindle", "--version"])
                .err()
                .unwrap()
                .kind(),
            clap::error::ErrorKind::DisplayVersion
        );
    }
    #[test]
    fn global_state_dir_works_before_and_after_subcommand() {
        assert_eq!(parse(&["list"]).state_dir, PathBuf::from(".state"));
        for args in [
            vec!["--state-dir", "custom", "list"],
            vec!["list", "--state-dir", "custom"],
        ] {
            let cli = parse(&args);
            assert_eq!(cli.state_dir, PathBuf::from("custom"));
            assert!(matches!(
                cli.command.into_request().unwrap(),
                Request::List {}
            ));
        }
    }
    #[test]
    fn service_commands_are_not_accidentally_forwarded_to_host() {
        for name in ["host", "api"] {
            assert!(parse(&[name]).command.into_request().is_err());
        }
        assert!(matches!(
            parse(&["health"]).command.into_request().unwrap(),
            Request::Health {}
        ));
    }
    #[test]
    fn cli_rejects_missing_commands_arguments_and_invalid_numbers() {
        for args in [
            vec![],
            vec!["unknown"],
            vec!["launch"],
            vec!["input", "id", "gen"],
            vec!["output", "id", "gen", "--after", "-1"],
            vec!["output", "id", "gen", "--after", "18446744073709551616"],
            vec!["resize", "id", "gen", "65536", "80"],
            vec!["cancel", "id"],
        ] {
            assert!(Cli::try_parse_from(std::iter::once("spindle").chain(args)).is_err());
        }
    }
    #[test]
    fn launch_keeps_child_flags_and_shell_text_literal() {
        let cli = parse(&["launch", "program", "--child-flag", "a b", "$(literal)"]);
        let Request::Launch { launch } = cli.command.into_request().unwrap() else {
            panic!()
        };
        assert_eq!(launch.program, "program");
        assert_eq!(launch.args, ["--child-flag", "a b", "$(literal)"]);
        uuid::Uuid::parse_str(&launch.id).unwrap();
        assert!(std::path::Path::new(&launch.cwd).is_absolute());
    }
    #[test]
    fn launch_uses_explicit_identity_and_resolves_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let id = "00000000-0000-4000-8000-000000000001";
        let Request::Launch { launch } = parse(&["launch", "--id", id, "--cwd", path, "program"])
            .command
            .into_request()
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(launch.id, id);
        assert_eq!(
            std::path::Path::new(&launch.cwd),
            dir.path().canonicalize().unwrap()
        );
        assert!(
            parse(&["launch", "--id", "invalid", "program"])
                .command
                .into_request()
                .is_err()
        );
        assert!(
            parse(&["launch", "--cwd", path, "program"])
                .command
                .into_request()
                .is_ok()
        );
        let missing = dir.path().join("missing");
        assert!(
            parse(&["launch", "--cwd", missing.to_str().unwrap(), "program"])
                .command
                .into_request()
                .is_err()
        );
    }
    #[test]
    fn controls_preserve_generation_bytes_dimensions_and_cursor() {
        let Request::Input {
            id,
            generation,
            data,
        } = parse(&["input", "id", "gen", "hello\n"])
            .command
            .into_request()
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            (id.as_str(), generation.as_str(), data.as_slice()),
            ("id", "gen", b"hello\n".as_slice())
        );
        let Request::Resize { rows, cols, .. } = parse(&["resize", "id", "gen", "42", "120"])
            .command
            .into_request()
            .unwrap()
        else {
            panic!()
        };
        assert_eq!((rows, cols), (42, 120));
        let Request::Output { after, .. } = parse(&["output", "id", "gen"])
            .command
            .into_request()
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(after, 0);
        let Request::Output { after, .. } =
            parse(&["output", "id", "gen", "--after", "18446744073709551615"])
                .command
                .into_request()
                .unwrap()
        else {
            panic!()
        };
        assert_eq!(after, u64::MAX);
        let Request::Cancel { id, generation } = parse(&["cancel", "id", "gen"])
            .command
            .into_request()
            .unwrap()
        else {
            panic!()
        };
        assert_eq!((id.as_str(), generation.as_str()), ("id", "gen"));
    }
    #[cfg(unix)]
    #[test]
    fn launch_refuses_lossy_working_directory_identity() {
        use std::os::unix::ffi::OsStringExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(std::ffi::OsString::from_vec(vec![255]));
        std::fs::create_dir(&path).unwrap();
        let command = Command::Launch {
            id: None,
            cwd: Some(path),
            program: "program".into(),
            args: vec![],
        };
        assert!(
            command
                .into_request()
                .unwrap_err()
                .to_string()
                .contains("UTF-8")
        );
    }
    #[test]
    fn native_agent_presets_parse_and_forward_model_without_shell() {
        for (name, program) in [("claude", "claude"), ("codex", "codex")] {
            let Request::Launch { launch } = parse(&["agent", name, "--model", "model-a"])
                .command
                .into_request()
                .unwrap()
            else {
                panic!()
            };
            assert_eq!(launch.program, program);
            assert_eq!(launch.args, ["--model", "model-a"]);
        }
        let dir = tempfile::tempdir().unwrap();
        let id = "00000000-0000-4000-8000-000000000001";
        let Request::Launch { launch } = parse(&[
            "agent",
            "codex",
            "--id",
            id,
            "--cwd",
            dir.path().to_str().unwrap(),
        ])
        .command
        .into_request()
        .unwrap() else {
            panic!()
        };
        assert_eq!(launch.id, id);
        assert_eq!(
            PathBuf::from(launch.cwd),
            dir.path().canonicalize().unwrap()
        );
        assert!(
            parse(&[
                "agent",
                "claude",
                "--cwd",
                dir.path().join("missing").to_str().unwrap()
            ])
            .command
            .into_request()
            .is_err()
        );
        assert!(Cli::try_parse_from(["spindle", "agent", "unknown"]).is_err());
        assert!(Cli::try_parse_from(["spindle", "agent"]).is_err());
    }
    #[test]
    fn api_routes_require_explicit_paid_opt_in_and_valid_secret_reference() {
        for provider in ["openai", "anthropic"] {
            assert!(
                prepare_ask(provider, "model".into(), "hello".into(), 10, false, None).is_err()
            );
            let (r, name) =
                prepare_ask(provider, "model".into(), "hello".into(), 10, true, None).unwrap();
            assert!(!r.free_only);
            assert_eq!(
                name,
                if provider == "openai" {
                    "OPENAI_API_KEY"
                } else {
                    "ANTHROPIC_API_KEY"
                }
            );
        }
        let (r, name) = prepare_ask(
            "openrouter",
            "openrouter/free".into(),
            "hello".into(),
            1024,
            false,
            None,
        )
        .unwrap();
        assert!(r.free_only);
        assert_eq!(name, "OPENROUTER_API_KEY");
        assert!(
            prepare_ask(
                "openrouter",
                "paid/model".into(),
                "hello".into(),
                10,
                false,
                None
            )
            .is_err()
        );
        for name in [
            "",
            "1KEY",
            "KEY=secret",
            "KEY NAME",
            "KEY\n",
            &"A".repeat(129),
        ] {
            assert!(
                prepare_ask(
                    "openrouter",
                    "openrouter/free".into(),
                    "hello".into(),
                    1,
                    false,
                    Some(name.into())
                )
                .is_err()
            );
        }
        let (_, name) = prepare_ask(
            "openrouter",
            "vendor/model:free".into(),
            "hello".into(),
            1,
            false,
            Some("_MY_KEY2".into()),
        )
        .unwrap();
        assert_eq!(name, "_MY_KEY2");
        assert!(prepare_ask("other", "model".into(), "hello".into(), 1, true, None).is_err());
    }
    #[test]
    fn ask_parser_requires_prompt_file_and_never_becomes_host_request() {
        let cli = parse(&[
            "ask",
            "--provider",
            "openrouter",
            "--model",
            "openrouter/free",
            "--prompt-file",
            "prompt.txt",
        ]);
        match &cli.command {
            Command::Ask {
                max_output_tokens,
                allow_paid,
                api_key_env,
                ..
            } => {
                assert_eq!(*max_output_tokens, 1024);
                assert!(!allow_paid);
                assert!(api_key_env.is_none());
            }
            _ => panic!(),
        }
        assert!(cli.command.into_request().is_err());
        for args in [
            vec!["ask"],
            vec!["ask", "--provider", "openai", "--model", "m"],
            vec!["ask", "--api-key", "secret"],
        ] {
            assert!(Cli::try_parse_from(std::iter::once("spindle").chain(args)).is_err());
        }
    }
    #[test]
    fn prompt_file_bounds_utf8_and_file_errors_are_checked_before_network() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt.txt");
        assert!(read_prompt(&path).is_err());
        assert!(read_prompt(dir.path()).is_err());
        for (bytes, ok) in [
            (vec![b'a'; 65536], true),
            (vec![b'a'; 65537], false),
            (vec![255], false),
            (b"hello\n".to_vec(), true),
            (vec![], true),
        ] {
            std::fs::write(&path, &bytes).unwrap();
            assert_eq!(read_prompt(&path).is_ok(), ok);
        }
        // Empty contents are a file read success but a request validation error.
        assert!(
            prepare_ask(
                "openrouter",
                "openrouter/free".into(),
                read_prompt(&path).unwrap(),
                1024,
                false,
                None
            )
            .is_err()
        );
    }
    #[cfg(unix)]
    #[test]
    fn prompt_special_files_are_refused_without_waiting_for_a_writer() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fifo");
        let raw = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(raw.as_ptr(), 0o600) }, 0);
        assert!(read_prompt(&path).is_err());
        let fifo_link = dir.path().join("fifo-link");
        std::os::unix::fs::symlink(&path, &fifo_link).unwrap();
        assert!(read_prompt(&fifo_link).is_err());
        let target = dir.path().join("regular");
        std::fs::write(&target, b"hello").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(read_prompt(&link).unwrap(), "hello");
    }
}
