//! Native CLI launch preparation. Each CLI retains its own login and permissions.
use crate::model::Launch;
use anyhow::{Context, Result, ensure};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum NativeEngine {
    Claude,
    Codex,
}

/// Construct a direct executable/argument launch without invoking a shell.
///
/// This does not discover credentials, change CLI configuration, or add flags
/// that bypass an engine's permission checks. Executable lookup and login are
/// left to the normal CLI environment when the host starts the process.
pub fn prepare_launch(
    engine: NativeEngine,
    id: Option<String>,
    cwd: PathBuf,
    model: Option<String>,
) -> Result<Launch> {
    let args = if let Some(model) = model {
        ensure!(
            !model.is_empty()
                && model.len() <= 256
                && !model.starts_with('-')
                && !model.chars().any(|c| c.is_whitespace() || c.is_control()),
            "model must be 1-256 bytes without whitespace, control characters, or a leading dash"
        );
        vec!["--model".into(), model]
    } else {
        Vec::new()
    };
    let cwd = cwd.canonicalize().context("resolve working directory")?;
    ensure!(cwd.is_dir(), "working directory must be a directory");
    let cwd = cwd.into_os_string().into_string().map_err(|_| {
        anyhow::anyhow!("working directory must be valid UTF-8; refusing a lossy path")
    })?;
    let launch = Launch {
        id: id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        program: match engine {
            NativeEngine::Claude => "claude",
            NativeEngine::Codex => "codex",
        }
        .into(),
        args,
        cwd,
    };
    launch.validate()?;
    Ok(launch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_engines_preserve_cli_defaults_without_extra_flags() {
        let dir = tempfile::tempdir().unwrap();
        for (engine, program) in [
            (NativeEngine::Claude, "claude"),
            (NativeEngine::Codex, "codex"),
        ] {
            let launch = prepare_launch(engine, None, dir.path().into(), None).unwrap();
            assert_eq!(launch.program, program);
            assert!(launch.args.is_empty());
            assert_eq!(
                PathBuf::from(launch.cwd),
                dir.path().canonicalize().unwrap()
            );
        }
    }

    #[test]
    fn model_is_one_literal_argument_for_both_engines() {
        let dir = tempfile::tempdir().unwrap();
        // Shell metacharacters remain literal argv bytes, never shell syntax.
        let model = "provider/model;$(literal)`text`";
        for engine in [NativeEngine::Claude, NativeEngine::Codex] {
            let launch =
                prepare_launch(engine, None, dir.path().into(), Some(model.into())).unwrap();
            assert_eq!(launch.args, ["--model", model]);
        }
    }

    #[test]
    fn explicit_id_is_preserved_and_missing_ids_are_distinct_uuids() {
        let dir = tempfile::tempdir().unwrap();
        let expected = uuid::Uuid::new_v4().to_string();
        let explicit = prepare_launch(
            NativeEngine::Claude,
            Some(expected.clone()),
            dir.path().into(),
            None,
        )
        .unwrap();
        assert_eq!(explicit.id, expected);
        let first = prepare_launch(NativeEngine::Codex, None, dir.path().into(), None).unwrap();
        let second = prepare_launch(NativeEngine::Codex, None, dir.path().into(), None).unwrap();
        assert!(uuid::Uuid::parse_str(&first.id).is_ok());
        assert!(uuid::Uuid::parse_str(&second.id).is_ok());
        assert_ne!(first.id, second.id);
        assert!(
            prepare_launch(
                NativeEngine::Codex,
                Some("not-an-id".into()),
                dir.path().into(),
                None
            )
            .is_err()
        );
    }

    #[test]
    fn invalid_model_names_are_rejected_without_normalizing_them() {
        let dir = tempfile::tempdir().unwrap();
        for model in [
            "",
            "-model",
            "two words",
            "trailing ",
            "\tmodel",
            "model\n",
            "model\0",
            "model\u{7f}",
            "model\u{a0}name",
        ] {
            assert!(
                prepare_launch(
                    NativeEngine::Claude,
                    None,
                    dir.path().into(),
                    Some(model.into())
                )
                .is_err(),
                "accepted {model:?}"
            );
        }
        assert!(
            prepare_launch(
                NativeEngine::Codex,
                None,
                dir.path().into(),
                Some("a".repeat(256))
            )
            .is_ok()
        );
        assert!(
            prepare_launch(
                NativeEngine::Codex,
                None,
                dir.path().into(),
                Some("a".repeat(257))
            )
            .is_err()
        );
        assert!(
            prepare_launch(
                NativeEngine::Codex,
                None,
                dir.path().into(),
                Some("é".repeat(129))
            )
            .is_err()
        );
    }

    #[test]
    fn missing_working_directory_and_regular_file_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            prepare_launch(NativeEngine::Claude, None, dir.path().join("absent"), None).is_err()
        );
        let file = dir.path().join("file");
        std::fs::write(&file, b"preserve").unwrap();
        assert!(prepare_launch(NativeEngine::Codex, None, file.clone(), None).is_err());
        assert_eq!(std::fs::read(file).unwrap(), b"preserve");
    }

    #[test]
    fn working_directory_is_canonicalized_without_changing_valid_unicode() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("project-é");
        std::fs::create_dir(&nested).unwrap();
        let launch = prepare_launch(NativeEngine::Codex, None, nested.join(".."), None).unwrap();
        assert_eq!(
            PathBuf::from(launch.cwd),
            dir.path().canonicalize().unwrap()
        );
        let launch = prepare_launch(NativeEngine::Claude, None, nested.clone(), None).unwrap();
        assert_eq!(PathBuf::from(launch.cwd), nested.canonicalize().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_working_directory_is_rejected_instead_of_replaced() {
        use std::os::unix::ffi::OsStringExt;
        let dir = tempfile::tempdir().unwrap();
        let invalid = dir
            .path()
            .join(std::ffi::OsString::from_vec(vec![b'p', 0xff]));
        std::fs::create_dir(&invalid).unwrap();
        let error = prepare_launch(NativeEngine::Claude, None, invalid.clone(), None).unwrap_err();
        assert!(error.to_string().contains("lossy path"));
        assert!(invalid.is_dir());
    }
}
