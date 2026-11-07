//! A bounded mailbox owns the sole SQLite connection on a dedicated thread.
use crate::model::{Launch, Session, Status};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::path::Path;
use tokio::sync::{mpsc, oneshot};

enum Command {
    List(oneshot::Sender<Result<Vec<Session>>>),
    Create(Launch, oneshot::Sender<Result<Session>>),
    Set(
        String,
        String,
        Status,
        Option<u32>,
        Option<String>,
        oneshot::Sender<Result<()>>,
    ),
}
#[derive(Clone)]
pub struct Store(mpsc::Sender<Command>);
impl Store {
    pub async fn open(path: &Path) -> Result<Self> {
        let path = path.to_owned();
        let (tx, mut rx) = mpsc::channel(64);
        let (ready_tx, ready_rx) = oneshot::channel();
        std::thread::spawn(move || {
            let db = (|| -> Result<Connection> {
                let mut db = Connection::open(path)?;
                initialize(&mut db)?;
                Ok(db)
            })();
            let db = match db {
                Ok(db) => {
                    let _ = ready_tx.send(Ok(()));
                    db
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            while let Some(command) = rx.blocking_recv() {
                match command {
                    Command::List(reply) => {
                        let _ = reply.send(list(&db));
                    }
                    Command::Create(launch, reply) => {
                        let result = create(&db, launch);
                        let _ = reply.send(result);
                    }
                    Command::Set(id, generation, status, code, error, reply) => {
                        let result = set(&db, &id, &generation, status, code, error);
                        let _ = reply.send(result);
                    }
                }
            }
        });
        ready_rx.await??;
        Ok(Self(tx))
    }
    pub async fn list(&self) -> Result<Vec<Session>> {
        let (tx, rx) = oneshot::channel();
        self.0.send(Command::List(tx)).await?;
        rx.await?
    }
    pub async fn create(&self, launch: Launch) -> Result<Session> {
        let (tx, rx) = oneshot::channel();
        self.0.send(Command::Create(launch, tx)).await?;
        rx.await?
    }
    pub async fn set(
        &self,
        id: &str,
        generation: &str,
        status: Status,
        code: Option<u32>,
        error: Option<String>,
    ) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.0
            .send(Command::Set(
                id.into(),
                generation.into(),
                status,
                code,
                error,
                tx,
            ))
            .await?;
        rx.await?
    }
}
fn list(db: &Connection) -> Result<Vec<Session>> {
    let mut stmt = db.prepare("SELECT value FROM sessions ORDER BY id")?;
    let values = stmt.query_map([], |row| row.get::<_, String>(0))?;
    values.map(|v| Ok(serde_json::from_str(&v?)?)).collect()
}
fn save(db: &Connection, session: &Session) -> Result<()> {
    db.execute("INSERT INTO sessions(id,value) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET value=excluded.value",params![session.launch.id,serde_json::to_string(session)?])?;
    Ok(())
}

const SCHEMA_VERSION: u32 = 1;

fn initialize(db: &mut Connection) -> Result<()> {
    // Inspect before any migration or pragma that changes the database. An older
    // executable must never downgrade the version stamp of a newer database.
    let version: u32 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    anyhow::ensure!(
        version <= SCHEMA_VERSION,
        "database schema {version} is newer than supported schema {SCHEMA_VERSION}"
    );
    db.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA busy_timeout=3000;",
    )?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if version == 0 {
        tx.execute_batch("CREATE TABLE IF NOT EXISTS sessions(id TEXT PRIMARY KEY, value TEXT NOT NULL); PRAGMA user_version=1;")?;
    }
    // Recovery is one atomic decision: a corrupt record or failed write cannot
    // leave half the sessions recovered. Never signal a PID from stored data.
    for mut session in list(&tx)? {
        if matches!(
            session.status,
            Status::Starting | Status::Running | Status::Cancelling
        ) {
            session.status = Status::Interrupted;
            session.error = Some("host restarted; live terminal cannot be recovered".into());
            session.exit_code = None;
            save(&tx, &session)?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn get(db: &Connection, id: &str) -> Result<Option<Session>> {
    let value: Option<String> = db
        .query_row("SELECT value FROM sessions WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .optional()?;
    value
        .map(|value| Ok(serde_json::from_str(&value)?))
        .transpose()
}

fn create(db: &Connection, launch: Launch) -> Result<Session> {
    if let Some(existing) = get(db, &launch.id)? {
        anyhow::ensure!(
            existing.launch == launch,
            "id already used for a different launch"
        );
        return Ok(existing);
    }
    launch.validate()?;
    let session = Session {
        launch,
        generation: uuid::Uuid::new_v4().to_string(),
        status: Status::Starting,
        exit_code: None,
        error: None,
    };
    // INSERT rather than upsert: creation must never replace a generation.
    db.execute(
        "INSERT INTO sessions(id,value) VALUES(?1,?2)",
        params![session.launch.id, serde_json::to_string(&session)?],
    )?;
    Ok(session)
}

fn legal_transition(from: &Status, to: &Status) -> bool {
    matches!(
        (from, to),
        (
            Status::Starting,
            Status::Running | Status::Failed | Status::Interrupted
        ) | (
            Status::Running,
            Status::Cancelling | Status::Exited | Status::Failed | Status::Interrupted
        ) | (
            Status::Cancelling,
            Status::Exited | Status::Failed | Status::Interrupted
        )
    )
}

fn set(
    db: &Connection,
    id: &str,
    generation: &str,
    status: Status,
    code: Option<u32>,
    error: Option<String>,
) -> Result<()> {
    let mut session = get(db, id)?.ok_or_else(|| anyhow::anyhow!("session not found"))?;
    anyhow::ensure!(session.generation == generation, "stale generation");
    // Exact replays are harmless, but a terminal result cannot be rewritten.
    if session.status == status && session.exit_code == code && session.error == error {
        return Ok(());
    }
    anyhow::ensure!(
        legal_transition(&session.status, &status),
        "illegal session transition {:?} -> {:?}",
        session.status,
        status
    );
    session.status = status;
    session.exit_code = code;
    session.error = error;
    save(db, &session)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn launch() -> Launch {
        Launch {
            id: uuid::Uuid::new_v4().to_string(),
            program: "test-program".into(),
            args: vec![],
            cwd: std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        }
    }
    fn database() -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        initialize(&mut db).unwrap();
        db
    }
    #[test]
    fn newer_schema_is_rejected_without_overwriting_version() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA user_version=999; CREATE TABLE future(value TEXT); INSERT INTO future VALUES('preserved');").unwrap();
        assert!(
            initialize(&mut db)
                .unwrap_err()
                .to_string()
                .contains("newer")
        );
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            999
        );
        assert_eq!(
            db.query_row("SELECT value FROM future", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "preserved"
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='sessions'",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
            0
        );
    }
    #[test]
    fn duplicate_launch_reuses_generation_but_collision_is_refused() {
        let db = database();
        let original = launch();
        let first = create(&db, original.clone()).unwrap();
        let replay = create(&db, original.clone()).unwrap();
        assert_eq!(first.generation, replay.generation);
        let mut collision = original;
        collision.args.push("different".into());
        assert!(create(&db, collision).is_err());
        assert_eq!(list(&db).unwrap().len(), 1);
    }
    #[test]
    fn stale_generation_and_illegal_transitions_preserve_state() {
        let db = database();
        let session = create(&db, launch()).unwrap();
        let id = &session.launch.id;
        assert!(set(&db, id, "stale", Status::Running, None, None).is_err());
        assert!(set(&db, id, &session.generation, Status::Exited, Some(0), None).is_err());
        assert_eq!(get(&db, id).unwrap().unwrap().status, Status::Starting);
        set(&db, id, &session.generation, Status::Running, None, None).unwrap();
        set(&db, id, &session.generation, Status::Cancelling, None, None).unwrap();
        set(&db, id, &session.generation, Status::Exited, Some(0), None).unwrap();
        set(&db, id, &session.generation, Status::Exited, Some(0), None).unwrap();
        assert!(set(&db, id, &session.generation, Status::Running, None, None).is_err());
        assert!(set(&db, id, &session.generation, Status::Exited, Some(1), None).is_err());
        assert_eq!(get(&db, id).unwrap().unwrap().exit_code, Some(0));
    }
    #[test]
    fn recovery_interrupts_active_records_and_preserves_terminal_records() {
        let mut db = database();
        for status in [
            Status::Starting,
            Status::Running,
            Status::Cancelling,
            Status::Exited,
            Status::Failed,
            Status::Interrupted,
        ] {
            let mut session = create(&db, launch()).unwrap();
            session.status = status;
            save(&db, &session).unwrap();
        }
        initialize(&mut db).unwrap();
        let sessions = list(&db).unwrap();
        assert_eq!(
            sessions
                .iter()
                .filter(|s| s.status == Status::Interrupted)
                .count(),
            4
        );
        assert_eq!(
            sessions
                .iter()
                .filter(|s| s.status == Status::Exited)
                .count(),
            1
        );
        assert_eq!(
            sessions
                .iter()
                .filter(|s| s.status == Status::Failed)
                .count(),
            1
        );
    }
    #[test]
    fn recovery_rolls_back_all_changes_when_a_write_fails() {
        let mut db = database();
        let a = create(&db, launch()).unwrap();
        let b = create(&db, launch()).unwrap();
        let last_id = if a.launch.id > b.launch.id {
            &a.launch.id
        } else {
            &b.launch.id
        };
        db.execute_batch(&format!("CREATE TRIGGER reject_recovery BEFORE UPDATE ON sessions WHEN OLD.id='{}' BEGIN SELECT RAISE(ABORT, 'injected failure'); END;", last_id)).unwrap();
        assert!(initialize(&mut db).is_err());
        assert!(
            list(&db)
                .unwrap()
                .iter()
                .all(|s| s.status == Status::Starting)
        );
    }
}
