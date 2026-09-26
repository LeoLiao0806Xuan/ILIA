use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::Connection;

use crate::{Database, DatabaseError};

pub const USER_SCHEMA_VERSION: i64 = 1;
pub const WORKSPACE_SCHEMA_VERSION: i64 = 1;

const USER_SCHEMA_V1: &str = r#"
CREATE TABLE user_documents (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    language TEXT NOT NULL,
    document_type TEXT NOT NULL,
    source_filename TEXT NOT NULL,
    source_sha256 TEXT NOT NULL UNIQUE CHECK (length(source_sha256) = 64),
    byte_length INTEGER NOT NULL CHECK (byte_length > 0),
    import_status TEXT NOT NULL CHECK (import_status IN ('preparing', 'ready', 'failed')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE user_document_aliases (
    id INTEGER PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES user_documents(id) ON DELETE CASCADE,
    language TEXT NOT NULL,
    alias TEXT NOT NULL,
    UNIQUE(document_id, language, alias)
);
CREATE TABLE user_chunks (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES user_documents(id) ON DELETE CASCADE,
    sequence_number INTEGER NOT NULL CHECK (sequence_number > 0),
    citation_label TEXT NOT NULL,
    language TEXT NOT NULL,
    text_original TEXT NOT NULL,
    text_normalized TEXT NOT NULL,
    UNIQUE(document_id, sequence_number)
);
CREATE VIRTUAL TABLE user_chunks_fts USING fts5(
    chunk_id UNINDEXED,
    document_id UNINDEXED,
    title,
    citation_label,
    text,
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TABLE user_embeddings (
    chunk_id TEXT NOT NULL REFERENCES user_chunks(id) ON DELETE CASCADE,
    model_id TEXT NOT NULL,
    dimension INTEGER NOT NULL CHECK (dimension > 0),
    vector BLOB NOT NULL,
    vector_norm REAL NOT NULL CHECK (vector_norm > 0),
    PRIMARY KEY(chunk_id, model_id)
);
CREATE TABLE import_jobs (
    id TEXT PRIMARY KEY,
    source_filename TEXT NOT NULL,
    source_sha256 TEXT,
    stage TEXT NOT NULL,
    progress REAL NOT NULL DEFAULT 0 CHECK (progress >= 0 AND progress <= 1),
    error_code TEXT,
    temporary_path TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_user_chunks_document ON user_chunks(document_id, sequence_number);
CREATE INDEX idx_user_embeddings_model ON user_embeddings(model_id, chunk_id);
"#;

const WORKSPACE_SCHEMA_V1: &str = r#"
CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE tags (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);
CREATE TABLE project_tags (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY(project_id, tag_id)
);
CREATE TABLE conversations (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
    content TEXT NOT NULL,
    sequence_number INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(conversation_id, sequence_number)
);
CREATE TABLE message_citations (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    evidence_number INTEGER NOT NULL CHECK (evidence_number > 0),
    stable_evidence_key TEXT NOT NULL,
    citation_label TEXT NOT NULL,
    PRIMARY KEY(message_id, evidence_number)
);
CREATE TABLE saved_evidence (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    stable_evidence_key TEXT NOT NULL,
    title TEXT NOT NULL,
    citation_label TEXT NOT NULL,
    text_snapshot TEXT NOT NULL,
    source_status TEXT NOT NULL DEFAULT 'available' CHECK (source_status IN ('available', 'deleted')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE notes (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    message_id TEXT REFERENCES messages(id) ON DELETE SET NULL,
    evidence_id TEXT REFERENCES saved_evidence(id) ON DELETE SET NULL,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE memo_sections (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    heading TEXT NOT NULL,
    body TEXT NOT NULL,
    sequence_number INTEGER NOT NULL,
    UNIQUE(project_id, sequence_number)
);
CREATE INDEX idx_conversations_project ON conversations(project_id, updated_at);
CREATE INDEX idx_saved_evidence_project ON saved_evidence(project_id, created_at);
CREATE INDEX idx_notes_project ON notes(project_id, updated_at);
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationDatabasePaths {
    pub core: PathBuf,
    pub user: PathBuf,
    pub workspace: PathBuf,
}

impl ApplicationDatabasePaths {
    pub fn initialize(
        core_database: impl AsRef<Path>,
        app_data_dir: impl AsRef<Path>,
    ) -> Result<Self, DatabaseError> {
        let core = core_database.as_ref().to_path_buf();
        Database::open_read_only(&core)?;
        let app_data_dir = app_data_dir.as_ref();
        std::fs::create_dir_all(app_data_dir)?;
        let user = app_data_dir.join("user.sqlite");
        let workspace = app_data_dir.join("workspace.sqlite");
        migrate(&user, "user", USER_SCHEMA_VERSION, USER_SCHEMA_V1)?;
        migrate(
            &workspace,
            "workspace",
            WORKSPACE_SCHEMA_VERSION,
            WORKSPACE_SCHEMA_V1,
        )?;
        Ok(Self {
            core,
            user,
            workspace,
        })
    }
}

fn migrate(
    path: &Path,
    database_name: &'static str,
    supported_version: i64,
    migration_v1: &str,
) -> Result<(), DatabaseError> {
    let mut connection = Connection::open(path)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (\
         version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);",
    )?;
    let current = connection
        .query_row("SELECT max(version) FROM schema_migrations", [], |row| {
            row.get::<_, Option<i64>>(0)
        })?
        .unwrap_or(0);
    if current > supported_version {
        return Err(DatabaseError::UnsupportedWritableSchema {
            database: database_name,
            found: current,
            supported: supported_version,
        });
    }
    if current < 1 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(migration_v1)?;
        transaction.execute("INSERT INTO schema_migrations(version) VALUES (1)", [])?;
        transaction.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

    fn temporary_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ilia-database-test-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn create_core(path: &Path) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_metadata (schema_version TEXT PRIMARY KEY, applied_at TEXT NOT NULL);\
                 INSERT INTO schema_metadata VALUES ('001', CURRENT_TIMESTAMP);",
            )
            .unwrap();
    }

    fn schema_version(path: &Path) -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row("SELECT max(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    #[test]
    fn initializes_three_isolated_databases_idempotently() {
        let temp = temporary_directory();
        let core = temp.join("core.sqlite");
        let app_data = temp.join("app-data");
        create_core(&core);
        let core_before = fs::read(&core).unwrap();

        let first = ApplicationDatabasePaths::initialize(&core, &app_data).unwrap();
        let second = ApplicationDatabasePaths::initialize(&core, &app_data).unwrap();

        assert_eq!(first, second);
        assert_eq!(schema_version(&first.user), USER_SCHEMA_VERSION);
        assert_eq!(schema_version(&first.workspace), WORKSPACE_SCHEMA_VERSION);
        assert_eq!(fs::read(&core).unwrap(), core_before);
        assert_ne!(first.core, first.user);
        assert_ne!(first.user, first.workspace);
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn refuses_a_newer_writable_schema() {
        let temp = temporary_directory();
        let core = temp.join("core.sqlite");
        let app_data = temp.join("app-data");
        create_core(&core);
        let paths = ApplicationDatabasePaths::initialize(&core, &app_data).unwrap();
        Connection::open(&paths.user)
            .unwrap()
            .execute("INSERT INTO schema_migrations(version) VALUES (99)", [])
            .unwrap();

        let error = ApplicationDatabasePaths::initialize(&core, &app_data).unwrap_err();
        assert!(matches!(
            error,
            DatabaseError::UnsupportedWritableSchema {
                database: "user",
                found: 99,
                supported: USER_SCHEMA_VERSION
            }
        ));
        fs::remove_dir_all(temp).unwrap();
    }
}
