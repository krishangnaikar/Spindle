use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const OUTPUT_LIMIT: usize = 256 * 1024;
pub const FRAME_LIMIT: usize = 64 * 1024;
pub const MAX_SESSIONS: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub id: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: String,
}
impl Launch {
    pub fn validate(&self) -> anyhow::Result<()> {
        uuid::Uuid::parse_str(&self.id)?;
        anyhow::ensure!(
            !self.program.is_empty() && !self.program.contains('\0'),
            "invalid executable"
        );
        anyhow::ensure!(
            std::path::Path::new(&self.cwd).is_absolute(),
            "cwd must be absolute"
        );
        anyhow::ensure!(self.args.len() <= 256, "too many arguments");
        anyhow::ensure!(
            !self.cwd.contains('\0') && self.args.iter().all(|arg| !arg.contains('\0')),
            "arguments and cwd cannot contain NUL"
        );
        anyhow::ensure!(
            serde_json::to_vec(self)?.len() <= 32 * 1024,
            "launch exceeds limit"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Starting,
    Running,
    Cancelling,
    Exited,
    Failed,
    Interrupted,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub launch: Launch,
    pub generation: String,
    pub status: Status,
    pub exit_code: Option<u32>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Health {},
    Launch {
        launch: Launch,
    },
    List {},
    Output {
        id: String,
        generation: String,
        after: u64,
    },
    Input {
        id: String,
        generation: String,
        data: Vec<u8>,
    },
    Resize {
        id: String,
        generation: String,
        rows: u16,
        cols: u16,
    },
    Cancel {
        id: String,
        generation: String,
    },
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
impl Reply {
    pub fn from_result(result: anyhow::Result<serde_json::Value>) -> Self {
        match result {
            Ok(value) => Self {
                ok: true,
                value: Some(value),
                error: None,
            },
            Err(error) => Self {
                ok: false,
                value: None,
                error: Some(error.to_string()),
            },
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Output {
    pub start: u64,
    pub next: u64,
    pub gap: bool,
    pub bytes: Vec<u8>,
    pub eof: bool,
}
#[derive(Default)]
pub struct Ring {
    bytes: VecDeque<u8>,
    next: u64,
    pub eof: bool,
}
impl Ring {
    pub fn append(&mut self, bytes: &[u8]) {
        self.next += bytes.len() as u64;
        if bytes.len() >= OUTPUT_LIMIT {
            self.bytes.clear();
            self.bytes.extend(&bytes[bytes.len() - OUTPUT_LIMIT..]);
        } else {
            let remove = (self.bytes.len() + bytes.len()).saturating_sub(OUTPUT_LIMIT);
            self.bytes.drain(..remove);
            self.bytes.extend(bytes);
        }
    }
    pub fn read(&self, after: u64) -> anyhow::Result<Output> {
        anyhow::ensure!(after <= self.next, "cursor ahead of output");
        let earliest = self.next - self.bytes.len() as u64;
        let start = after.max(earliest);
        // Bound each reply independently of retained history.
        let bytes: Vec<_> = self
            .bytes
            .iter()
            .skip((start - earliest) as usize)
            .take(8192)
            .copied()
            .collect();
        let next = start + bytes.len() as u64;
        Ok(Output {
            start,
            next,
            gap: after < earliest,
            bytes,
            eof: self.eof && next == self.next,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replay_reports_gap_and_supports_byte_cursor() {
        let mut ring = Ring::default();
        ring.append(&vec![b'a'; OUTPUT_LIMIT + 17]);
        let page = ring.read(0).unwrap();
        assert!(page.gap);
        assert_eq!(page.start, 17);
        assert_eq!(page.bytes.len(), 8192);
        assert!(!ring.read(page.next).unwrap().gap);
        assert!(ring.read((OUTPUT_LIMIT + 18) as u64).is_err());
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use serde_json::json;

    fn valid() -> Launch {
        Launch {
            id: "00000000-0000-4000-8000-000000000001".into(),
            program: "program".into(),
            args: vec![],
            cwd: std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        }
    }

    #[test]
    fn launch_validation_accepts_structured_literals_and_argument_limit() {
        let mut launch = valid();
        launch.args = vec!["$(not-a-shell-command); quoted spaces".into(); 256];
        launch.validate().unwrap();
        launch.args.push(String::new());
        assert!(launch.validate().is_err());
    }

    #[test]
    fn launch_validation_rejects_bad_identity_paths_and_nuls() {
        let mut cases = Vec::new();
        let mut launch = valid();
        launch.id = "not-a-uuid".into();
        cases.push(launch);
        let mut launch = valid();
        launch.program.clear();
        cases.push(launch);
        let mut launch = valid();
        launch.program = "prog\0ram".into();
        cases.push(launch);
        let mut launch = valid();
        launch.cwd = "relative/path".into();
        cases.push(launch);
        let mut launch = valid();
        launch.cwd.clear();
        cases.push(launch);
        let mut launch = valid();
        launch.cwd.push('\0');
        cases.push(launch);
        let mut launch = valid();
        launch.args.push("bad\0arg".into());
        cases.push(launch);
        for launch in cases {
            assert!(
                launch.validate().is_err(),
                "accepted invalid launch: {launch:?}"
            );
        }
    }

    #[test]
    fn launch_serialized_size_limit_is_inclusive() {
        let mut launch = valid();
        launch.args.push(String::new());
        let overhead = serde_json::to_vec(&launch).unwrap().len();
        launch.args[0] = "x".repeat(32 * 1024 - overhead);
        launch.validate().unwrap();
        launch.args[0].push('x');
        assert!(launch.validate().is_err());
        // Escaped bytes count towards the wire limit, not only string length.
        launch.args[0] = "\n".repeat(17000);
        assert!(launch.validate().is_err());
    }

    #[test]
    fn every_request_round_trips_without_losing_bytes_or_cursors() {
        let launch = valid();
        let id = launch.id.clone();
        let generation = "generation".to_string();
        let requests = vec![
            Request::Health {},
            Request::List {},
            Request::Launch { launch },
            Request::Output {
                id: id.clone(),
                generation: generation.clone(),
                after: u64::MAX,
            },
            Request::Input {
                id: id.clone(),
                generation: generation.clone(),
                data: vec![0, 10, 27, 128, 255],
            },
            Request::Resize {
                id: id.clone(),
                generation: generation.clone(),
                rows: 42,
                cols: 120,
            },
            Request::Cancel { id, generation },
        ];
        for request in requests {
            let value = serde_json::to_value(&request).unwrap();
            let decoded: Request = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), value);
        }
    }

    #[test]
    fn wire_rejects_unknown_missing_or_out_of_range_fields() {
        for invalid in [
            json!({"op":"unknown"}),
            json!({"op":"health","extra":true}),
            json!({"op":"list","extra":true}),
            json!({"op":"input","id":"id"}),
            json!({"op":"output","id":"id","generation":"g","after":-1}),
            json!({"op":"resize","id":"id","generation":"g","rows":65536,"cols":80}),
            json!({"op":"input","id":"id","generation":"g","data":[256]}),
            json!({"op":"cancel","id":"id","generation":"g","extra":true}),
        ] {
            assert!(
                serde_json::from_value::<Request>(invalid.clone()).is_err(),
                "accepted {invalid}"
            );
        }
        let mut value = serde_json::to_value(valid()).unwrap();
        value.as_object_mut().unwrap().remove("args");
        assert!(
            serde_json::from_value::<Launch>(value.clone())
                .unwrap()
                .args
                .is_empty()
        );
        value["extra"] = json!(true);
        assert!(serde_json::from_value::<Launch>(value).is_err());
    }

    #[test]
    fn reply_envelopes_keep_success_and_error_mutually_exclusive() {
        assert_eq!(
            serde_json::to_value(Reply::from_result(Ok(json!(null)))).unwrap(),
            json!({"ok":true,"value":null})
        );
        assert_eq!(
            serde_json::to_value(Reply::from_result(Err(anyhow::anyhow!("failure")))).unwrap(),
            json!({"ok":false,"error":"failure"})
        );
    }

    #[test]
    fn empty_and_closed_output_have_correct_eof_and_cursor() {
        let mut ring = Ring::default();
        let empty = ring.read(0).unwrap();
        assert!(empty.bytes.is_empty());
        assert!(!empty.eof);
        assert!(!empty.gap);
        ring.eof = true;
        assert!(ring.read(0).unwrap().eof);
        assert!(ring.read(1).is_err());
    }

    #[test]
    fn eof_only_appears_at_the_final_page_and_empty_append_is_harmless() {
        let mut ring = Ring::default();
        ring.append(&vec![255; 8193]);
        ring.append(&[]);
        ring.eof = true;
        let first = ring.read(0).unwrap();
        assert_eq!(first.next, 8192);
        assert!(!first.eof);
        let last = ring.read(first.next).unwrap();
        assert_eq!(last.bytes, vec![255]);
        assert!(last.eof);
        let end = ring.read(last.next).unwrap();
        assert!(end.bytes.is_empty());
        assert!(end.eof);
    }

    #[test]
    fn incremental_eviction_preserves_order_at_exact_capacity() {
        let mut ring = Ring::default();
        ring.append(&vec![1; OUTPUT_LIMIT]);
        assert!(!ring.read(0).unwrap().gap);
        ring.append(&[2, 3]);
        let first = ring.read(0).unwrap();
        assert_eq!(first.start, 2);
        assert!(first.gap);
        assert!(!ring.read(2).unwrap().gap);
        let end = ring.read(OUTPUT_LIMIT as u64).unwrap();
        assert_eq!(end.bytes, vec![2, 3]);
        ring.append(&vec![4; OUTPUT_LIMIT + 3]);
        let page = ring.read(0).unwrap();
        assert_eq!(page.start, (OUTPUT_LIMIT + 5) as u64);
        assert!(page.bytes.iter().all(|b| *b == 4));
    }

    #[test]
    fn arbitrary_chunks_match_a_reference_byte_stream() {
        let mut ring = Ring::default();
        let mut full = Vec::new();
        let mut seed = 7u64;
        for _ in 0..120 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let size = (seed as usize) % 9000;
            let chunk: Vec<u8> = (0..size)
                .map(|i| i.wrapping_add(seed as usize) as u8)
                .collect();
            full.extend_from_slice(&chunk);
            ring.append(&chunk);
            let earliest = full.len().saturating_sub(OUTPUT_LIMIT);
            for after in [0, earliest, full.len().saturating_sub(17), full.len()] {
                let output = ring.read(after as u64).unwrap();
                let start = after.max(earliest);
                let end = (start + 8192).min(full.len());
                assert_eq!(output.bytes, full[start..end]);
                assert_eq!(output.next, end as u64);
                assert_eq!(output.gap, after < earliest);
            }
        }
    }
}
