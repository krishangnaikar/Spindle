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
    #[test]
    fn every_status_pair_obeys_the_transition_contract() {
        let states = [
            Status::Starting,
            Status::Running,
            Status::Cancelling,
            Status::Exited,
            Status::Failed,
            Status::Interrupted,
        ];
        // Rows are source states, columns destination states, in the order above.
        // Diagonal entries exercise exact replay, not a fresh transition.
        let allowed = [
            [true, true, false, false, true, true],
            [false, true, true, true, true, true],
            [false, false, true, true, true, true],
            [false, false, false, true, false, false],
            [false, false, false, false, true, false],
            [false, false, false, false, false, true],
        ];
        let db = database();
        for (i, from) in states.iter().enumerate() {
            for (j, to) in states.iter().enumerate() {
                let mut record = create(&db, launch()).unwrap();
                record.status = from.clone();
                save(&db, &record).unwrap();
                let result = set(
                    &db,
                    &record.launch.id,
                    &record.generation,
                    to.clone(),
                    None,
                    None,
                );
                assert_eq!(
                    result.is_ok(),
                    allowed[i][j],
                    "{from:?} -> {to:?}: {result:?}"
                );
                let actual = get(&db, &record.launch.id).unwrap().unwrap();
                assert_eq!(
                    actual.status,
                    if allowed[i][j] {
                        to.clone()
                    } else {
                        from.clone()
                    }
                );
                assert_eq!(actual.generation, record.generation);
            }
        }
    }

    #[test]
    fn all_terminal_payloads_allow_exact_replay_but_reject_revisions() {
        let db = database();
        for status in [Status::Exited, Status::Failed, Status::Interrupted] {
            let mut record = create(&db, launch()).unwrap();
            record.status = status.clone();
            record.exit_code = Some(17);
            record.error = Some("original evidence".into());
            save(&db, &record).unwrap();
            assert!(
                set(
                    &db,
                    &record.launch.id,
                    "wrong-generation",
                    status.clone(),
                    record.exit_code,
                    record.error.clone()
                )
                .is_err()
            );
            set(
                &db,
                &record.launch.id,
                &record.generation,
                status.clone(),
                record.exit_code,
                record.error.clone(),
            )
            .unwrap();
            assert!(
                set(
                    &db,
                    &record.launch.id,
                    &record.generation,
                    status.clone(),
                    Some(18),
                    record.error.clone()
                )
                .is_err()
            );
            assert!(
                set(
                    &db,
                    &record.launch.id,
                    &record.generation,
                    status,
                    record.exit_code,
                    None
                )
                .is_err()
            );
            assert_eq!(
                serde_json::to_value(get(&db, &record.launch.id).unwrap().unwrap()).unwrap(),
                serde_json::to_value(record).unwrap()
            );
        }
    }

    #[test]
    fn missing_and_malformed_records_fail_without_overwriting_evidence() {
        let mut db = database();
        assert!(get(&db, "missing").unwrap().is_none());
        assert!(
            set(&db, "missing", "generation", Status::Running, None, None)
                .unwrap_err()
                .to_string()
                .contains("not found")
        );
        let good = create(&db, launch()).unwrap();
        let invalid = launch();
        db.execute(
            "INSERT INTO sessions(id,value) VALUES(?1,?2)",
            params![invalid.id, "{broken-json"],
        )
        .unwrap();
        assert!(get(&db, &invalid.id).is_err());
        assert!(list(&db).is_err());
        assert!(create(&db, invalid.clone()).is_err());
        assert!(set(&db, &invalid.id, "generation", Status::Running, None, None).is_err());
        assert!(initialize(&mut db).is_err());
        assert_eq!(
            get(&db, &good.launch.id).unwrap().unwrap().status,
            Status::Starting
        );
        assert_eq!(
            db.query_row(
                "SELECT value FROM sessions WHERE id=?1",
                [&invalid.id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "{broken-json"
        );
    }

    #[test]
    fn invalid_launch_never_creates_a_row() {
        let db = database();
        let mut cases = vec![];
        let mut invalid = launch();
        invalid.id = "not-a-uuid".into();
        cases.push(invalid);
        let mut invalid = launch();
        invalid.program.clear();
        cases.push(invalid);
        let mut invalid = launch();
        invalid.cwd = "relative/path".into();
        cases.push(invalid);
        let mut invalid = launch();
        invalid.args = vec!["arg".into(); 257];
        cases.push(invalid);
        for invalid in cases {
            assert!(create(&db, invalid).is_err());
        }
        assert!(list(&db).unwrap().is_empty());
    }

    #[test]
    fn disk_reopen_preserves_generation_and_terminal_payload_and_recovers_active() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let (active, terminal) = {
            let mut db = Connection::open(&path).unwrap();
            initialize(&mut db).unwrap();
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                    .unwrap(),
                SCHEMA_VERSION
            );
            let active = create(&db, launch()).unwrap();
            let terminal = create(&db, launch()).unwrap();
            set(
                &db,
                &terminal.launch.id,
                &terminal.generation,
                Status::Failed,
                None,
                Some("launch failed".into()),
            )
            .unwrap();
            (active, get(&db, &terminal.launch.id).unwrap().unwrap())
        };
        let mut db = Connection::open(&path).unwrap();
        initialize(&mut db).unwrap();
        let recovered = get(&db, &active.launch.id).unwrap().unwrap();
        assert_eq!(recovered.generation, active.generation);
        assert_eq!(recovered.status, Status::Interrupted);
        assert!(recovered.error.unwrap().contains("host restarted"));
        assert_eq!(
            serde_json::to_value(get(&db, &terminal.launch.id).unwrap().unwrap()).unwrap(),
            serde_json::to_value(&terminal).unwrap()
        );
        initialize(&mut db).unwrap();
        assert_eq!(list(&db).unwrap().len(), 2);
    }

    #[tokio::test]
    async fn actor_concurrent_duplicate_creation_is_one_generation() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("state.db")).await.unwrap();
        let original = launch();
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..80 {
            let store = store.clone();
            let launch = original.clone();
            tasks.spawn(async move { store.create(launch).await.unwrap() });
        }
        let mut generations = std::collections::HashSet::new();
        while let Some(result) = tasks.join_next().await {
            generations.insert(result.unwrap().generation);
        }
        assert_eq!(generations.len(), 1);
        let rows = store.list().await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].launch, original);
        store
            .set(
                &original.id,
                &rows[0].generation,
                Status::Running,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(store.list().await.unwrap()[0].status, Status::Running);
        let mut conflicting = original;
        conflicting.program = "other-program".into();
        assert!(store.create(conflicting).await.is_err());
        assert!(
            store
                .set("missing", "missing", Status::Failed, None, None)
                .await
                .is_err()
        );
        // A failed command does not stop the database worker.
        assert_eq!(store.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn actor_startup_errors_are_reported_without_hanging_or_rewriting() {
        let temp = tempfile::tempdir().unwrap();
        assert!(Store::open(temp.path()).await.is_err());
        let path = temp.path().join("future.db");
        {
            let db = Connection::open(&path).unwrap();
            db.execute_batch("PRAGMA user_version=999").unwrap();
        }
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), Store::open(&path))
            .await
            .unwrap();
        assert!(result.is_err());
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            999
        );
    }

    #[tokio::test]
    async fn actor_propagates_closed_mailbox_and_dropped_reply() {
        let (tx, rx) = mpsc::channel(1);
        let store = Store(tx);
        drop(rx);
        assert!(store.list().await.is_err());
        assert!(store.create(launch()).await.is_err());
        assert!(
            store
                .set("id", "generation", Status::Failed, None, None)
                .await
                .is_err()
        );
        let (tx, mut rx) = mpsc::channel(1);
        let store = Store(tx);
        let request = tokio::spawn(async move { store.list().await });
        drop(rx.recv().await.unwrap());
        assert!(request.await.unwrap().is_err());
    }
}
