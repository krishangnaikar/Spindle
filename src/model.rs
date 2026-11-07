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
    Health,
    Launch {
        launch: Launch,
    },
    List,
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
