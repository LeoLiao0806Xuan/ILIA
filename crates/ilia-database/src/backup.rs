use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{DatabaseError, USER_SCHEMA_VERSION, WORKSPACE_SCHEMA_VERSION};

const BACKUP_SCHEMA_VERSION: u32 = 1;
const MAX_DATABASE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupManifest {
    pub schema_version: u32,
    pub app_version: String,
    pub created_at_utc: String,
    pub user_schema_version: i64,
    pub workspace_schema_version: i64,
    pub user_sha256: String,
    pub workspace_sha256: String,
    pub user_bytes: u64,
    pub workspace_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupSummary {
    pub path: String,
    pub user_bytes: u64,
    pub workspace_bytes: u64,
    pub user_sha256: String,
    pub workspace_sha256: String,
}

pub fn create_workspace_backup(
    user_database: impl AsRef<Path>,
    workspace_database: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
    app_version: &str,
) -> Result<BackupSummary, DatabaseError> {
    let user_database = user_database.as_ref();
    let workspace_database = workspace_database.as_ref();
    let output_path = output_path.as_ref();
    if output_path.extension().and_then(|value| value.to_str()) != Some("ilia-workspace") {
        return Err(DatabaseError::InvalidWorkspace(
            "备份文件必须使用 .ilia-workspace 扩展名".into(),
        ));
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    checkpoint(user_database)?;
    checkpoint(workspace_database)?;
    validate_database(user_database, USER_SCHEMA_VERSION, "user")?;
    validate_database(workspace_database, WORKSPACE_SCHEMA_VERSION, "workspace")?;

    let user_bytes = fs::metadata(user_database)?.len();
    let workspace_bytes = fs::metadata(workspace_database)?.len();
    let user_sha256 = sha256_file(user_database)?;
    let workspace_sha256 = sha256_file(workspace_database)?;
    let manifest = BackupManifest {
        schema_version: BACKUP_SCHEMA_VERSION,
        app_version: app_version.to_owned(),
        created_at_utc: unix_timestamp().to_string(),
        user_schema_version: USER_SCHEMA_VERSION,
        workspace_schema_version: WORKSPACE_SCHEMA_VERSION,
        user_sha256: user_sha256.clone(),
        workspace_sha256: workspace_sha256.clone(),
        user_bytes,
        workspace_bytes,
    };
    let temporary = temporary_sibling(output_path, "backup");
    let result = (|| -> Result<(), DatabaseError> {
        let mut archive = ZipWriter::new(File::create(&temporary)?);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o600);
        archive
            .start_file("manifest.json", options)
            .map_err(zip_error)?;
        archive.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        add_file(&mut archive, "user.sqlite", user_database, options)?;
        add_file(
            &mut archive,
            "workspace.sqlite",
            workspace_database,
            options,
        )?;
        archive.finish().map_err(zip_error)?;
        replace_file(&temporary, output_path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(BackupSummary {
        path: output_path.display().to_string(),
        user_bytes,
        workspace_bytes,
        user_sha256,
        workspace_sha256,
    })
}

pub fn restore_workspace_backup(
    user_database: impl AsRef<Path>,
    workspace_database: impl AsRef<Path>,
    package_path: impl AsRef<Path>,
) -> Result<BackupSummary, DatabaseError> {
    let user_database = user_database.as_ref();
    let workspace_database = workspace_database.as_ref();
    let package_path = package_path.as_ref();
    let mut archive = ZipArchive::new(File::open(package_path)?).map_err(zip_error)?;
    if archive.len() != 3 {
        return Err(invalid("备份包必须且只能包含 manifest.json 和两个数据库"));
    }
    let manifest: BackupManifest = {
        let mut entry = archive.by_name("manifest.json").map_err(zip_error)?;
        if entry.size() > 1024 * 1024 {
            return Err(invalid("备份清单异常过大"));
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        serde_json::from_slice(&bytes)?
    };
    validate_manifest(&manifest)?;
    let staged_user = temporary_sibling(user_database, "restore-user");
    let staged_workspace = temporary_sibling(workspace_database, "restore-workspace");
    let result = (|| -> Result<BackupSummary, DatabaseError> {
        extract_entry(
            &mut archive,
            "user.sqlite",
            &staged_user,
            manifest.user_bytes,
        )?;
        extract_entry(
            &mut archive,
            "workspace.sqlite",
            &staged_workspace,
            manifest.workspace_bytes,
        )?;
        if sha256_file(&staged_user)? != manifest.user_sha256
            || sha256_file(&staged_workspace)? != manifest.workspace_sha256
        {
            return Err(invalid("备份数据库哈希校验失败"));
        }
        validate_database(&staged_user, USER_SCHEMA_VERSION, "user")?;
        validate_database(&staged_workspace, WORKSPACE_SCHEMA_VERSION, "workspace")?;
        checkpoint(user_database)?;
        checkpoint(workspace_database)?;
        replace_database_pair(
            user_database,
            workspace_database,
            &staged_user,
            &staged_workspace,
        )?;
        Ok(BackupSummary {
            path: package_path.display().to_string(),
            user_bytes: manifest.user_bytes,
            workspace_bytes: manifest.workspace_bytes,
            user_sha256: manifest.user_sha256,
            workspace_sha256: manifest.workspace_sha256,
        })
    })();
    let _ = fs::remove_file(&staged_user);
    let _ = fs::remove_file(&staged_workspace);
    result
}

fn validate_manifest(manifest: &BackupManifest) -> Result<(), DatabaseError> {
    if manifest.schema_version != BACKUP_SCHEMA_VERSION
        || manifest.user_schema_version != USER_SCHEMA_VERSION
        || manifest.workspace_schema_version != WORKSPACE_SCHEMA_VERSION
        || manifest.user_bytes > MAX_DATABASE_BYTES
        || manifest.workspace_bytes > MAX_DATABASE_BYTES
        || manifest.user_sha256.len() != 64
        || manifest.workspace_sha256.len() != 64
    {
        return Err(invalid("备份清单版本、大小或哈希无效"));
    }
    Ok(())
}

fn extract_entry(
    archive: &mut ZipArchive<File>,
    name: &str,
    destination: &Path,
    expected_bytes: u64,
) -> Result<(), DatabaseError> {
    let entry = archive.by_name(name).map_err(zip_error)?;
    if entry.name() != name || entry.size() != expected_bytes || entry.size() > MAX_DATABASE_BYTES {
        return Err(invalid("备份条目名称或大小不符合清单"));
    }
    let mut output = File::create(destination)?;
    std::io::copy(&mut entry.take(expected_bytes + 1), &mut output)?;
    output.sync_all()?;
    if fs::metadata(destination)?.len() != expected_bytes {
        return Err(invalid("备份条目长度不符合清单"));
    }
    Ok(())
}

fn add_file(
    archive: &mut ZipWriter<File>,
    name: &str,
    path: &Path,
    options: SimpleFileOptions,
) -> Result<(), DatabaseError> {
    archive.start_file(name, options).map_err(zip_error)?;
    let mut source = File::open(path)?;
    std::io::copy(&mut source, archive)?;
    Ok(())
}

fn checkpoint(path: &Path) -> Result<(), DatabaseError> {
    let connection = Connection::open(path)?;
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

fn validate_database(path: &Path, supported: i64, name: &'static str) -> Result<(), DatabaseError> {
    let connection = Connection::open(path)?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(invalid(format!("{name} 数据库完整性校验失败")));
    }
    let version = connection
        .query_row("SELECT max(version) FROM schema_migrations", [], |row| {
            row.get::<_, Option<i64>>(0)
        })?
        .unwrap_or(0);
    if version != supported {
        return Err(DatabaseError::UnsupportedWritableSchema {
            database: name,
            found: version,
            supported,
        });
    }
    Ok(())
}

fn replace_database_pair(
    user: &Path,
    workspace: &Path,
    staged_user: &Path,
    staged_workspace: &Path,
) -> Result<(), DatabaseError> {
    let old_user = temporary_sibling(user, "before-restore");
    let old_workspace = temporary_sibling(workspace, "before-restore");
    fs::rename(user, &old_user)?;
    if let Err(error) = fs::rename(workspace, &old_workspace) {
        let _ = fs::rename(&old_user, user);
        return Err(error.into());
    }
    let install =
        fs::rename(staged_user, user).and_then(|_| fs::rename(staged_workspace, workspace));
    if let Err(error) = install {
        let _ = fs::remove_file(user);
        let _ = fs::remove_file(workspace);
        let _ = fs::rename(&old_user, user);
        let _ = fs::rename(&old_workspace, workspace);
        return Err(error.into());
    }
    let _ = fs::remove_file(old_user);
    let _ = fs::remove_file(old_workspace);
    remove_sqlite_sidecars(user);
    remove_sqlite_sidecars(workspace);
    Ok(())
}

fn replace_file(source: &Path, destination: &Path) -> Result<(), DatabaseError> {
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(source, destination)?;
    Ok(())
}

fn remove_sqlite_sidecars(path: &Path) {
    let value = path.as_os_str().to_string_lossy();
    let _ = fs::remove_file(format!("{value}-wal"));
    let _ = fs::remove_file(format!("{value}-shm"));
}

fn sha256_file(path: &Path) -> Result<String, DatabaseError> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn temporary_sibling(path: &Path, label: &str) -> PathBuf {
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("ilia");
    path.with_file_name(format!(".{filename}.{label}.{}", Uuid::new_v4()))
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn invalid(message: impl Into<String>) -> DatabaseError {
    DatabaseError::InvalidWorkspace(message.into())
}

fn zip_error(error: zip::result::ZipError) -> DatabaseError {
    invalid(format!("备份包格式错误：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_database(path: &Path, marker: &str) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY);\
                 INSERT INTO schema_migrations VALUES (1);\
                 CREATE TABLE marker(value TEXT NOT NULL);",
            )
            .unwrap();
        connection
            .execute("INSERT INTO marker VALUES (?1)", [marker])
            .unwrap();
    }

    fn marker(path: &Path) -> String {
        Connection::open(path)
            .unwrap()
            .query_row("SELECT value FROM marker", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn backup_round_trip_restores_both_databases() {
        let root = std::env::temp_dir().join(format!("ilia-backup-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let user = root.join("user.sqlite");
        let workspace = root.join("workspace.sqlite");
        let package = root.join("research.ilia-workspace");
        create_database(&user, "before-user");
        create_database(&workspace, "before-workspace");
        create_workspace_backup(&user, &workspace, &package, "test").unwrap();
        Connection::open(&user)
            .unwrap()
            .execute("UPDATE marker SET value='changed'", [])
            .unwrap();
        Connection::open(&workspace)
            .unwrap()
            .execute("UPDATE marker SET value='changed'", [])
            .unwrap();
        restore_workspace_backup(&user, &workspace, &package).unwrap();
        assert_eq!(marker(&user), "before-user");
        assert_eq!(marker(&workspace), "before-workspace");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupted_backup_does_not_replace_current_databases() {
        let root = std::env::temp_dir().join(format!("ilia-backup-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let user = root.join("user.sqlite");
        let workspace = root.join("workspace.sqlite");
        let package = root.join("broken.ilia-workspace");
        create_database(&user, "current-user");
        create_database(&workspace, "current-workspace");
        fs::write(&package, b"not a zip").unwrap();
        assert!(restore_workspace_backup(&user, &workspace, &package).is_err());
        assert_eq!(marker(&user), "current-user");
        assert_eq!(marker(&workspace), "current-workspace");
        fs::remove_dir_all(root).unwrap();
    }
}
