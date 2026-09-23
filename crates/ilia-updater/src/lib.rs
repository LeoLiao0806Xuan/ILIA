use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ring::signature::{ED25519, UnparsedPublicKey};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const UPDATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] ureq::Error),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("invalid update manifest: {0}")]
    InvalidManifest(String),
    #[error("untrusted signing key: {0}")]
    UntrustedKey(String),
    #[error("signature verification failed")]
    InvalidSignature,
    #[error("payload verification failed for {component}: {reason}")]
    InvalidPayload { component: String, reason: String },
    #[error("update target escapes install root: {0}")]
    UnsafeTarget(String),
    #[error("component {component} requires installed version {expected}, found {actual}")]
    VersionMismatch {
        component: String,
        expected: String,
        actual: String,
    },
    #[error("update apply failed: {apply}; rollback errors: {rollback:?}")]
    ApplyAndRollback {
        apply: String,
        rollback: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedPublicKey {
    pub key_id: String,
    pub algorithm: String,
    pub public_key_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetachedSignature {
    pub key_id: String,
    pub algorithm: String,
    pub signature_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateManifest {
    pub schema_version: u32,
    pub release_id: String,
    pub channel: String,
    pub created_at: String,
    pub components: Vec<UpdateComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateComponent {
    pub id: String,
    pub kind: ComponentKind,
    pub version: String,
    #[serde(default)]
    pub from_version: Option<String>,
    pub target: String,
    pub payload_url: String,
    pub payload_size: u64,
    pub payload_sha256: String,
    pub payload_format: PayloadFormat,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    Application,
    Corpus,
    Model,
    Runtime,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PayloadFormat {
    RawFile,
    SqlitePatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct InstalledVersions {
    pub components: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SqlitePatch {
    pub schema_version: u32,
    pub statements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateJournal {
    pub release_id: String,
    pub status: JournalStatus,
    pub applied_components: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JournalStatus {
    Staged,
    Applying,
    Applied,
    RollingBack,
    RolledBack,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateOutcome {
    pub release_id: String,
    pub applied_components: Vec<String>,
    pub backup_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct VerifiedRelease {
    pub manifest: UpdateManifest,
    pub manifest_bytes: Vec<u8>,
}

pub fn verify_manifest(
    manifest_bytes: &[u8],
    signature_bytes: &[u8],
    trusted_key: &TrustedPublicKey,
) -> Result<VerifiedRelease, UpdateError> {
    if trusted_key.algorithm != "ed25519" {
        return Err(UpdateError::UntrustedKey(trusted_key.algorithm.clone()));
    }
    let detached: DetachedSignature = serde_json::from_slice(signature_bytes)?;
    if detached.key_id != trusted_key.key_id || detached.algorithm != "ed25519" {
        return Err(UpdateError::UntrustedKey(detached.key_id));
    }
    let public_key: [u8; 32] = STANDARD
        .decode(&trusted_key.public_key_base64)
        .map_err(|error| UpdateError::InvalidManifest(error.to_string()))?
        .try_into()
        .map_err(|_| UpdateError::InvalidManifest("Ed25519 public key must be 32 bytes".into()))?;
    let signature = STANDARD
        .decode(&detached.signature_base64)
        .map_err(|error| UpdateError::InvalidManifest(error.to_string()))?;
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(manifest_bytes, &signature)
        .map_err(|_| UpdateError::InvalidSignature)?;
    let manifest: UpdateManifest = serde_json::from_slice(manifest_bytes)?;
    validate_manifest(&manifest)?;
    Ok(VerifiedRelease {
        manifest,
        manifest_bytes: manifest_bytes.to_vec(),
    })
}

pub fn validate_manifest(manifest: &UpdateManifest) -> Result<(), UpdateError> {
    if manifest.schema_version != UPDATE_SCHEMA_VERSION {
        return Err(UpdateError::InvalidManifest(format!(
            "unsupported schema version {}",
            manifest.schema_version
        )));
    }
    if manifest.release_id.trim().is_empty() || manifest.components.is_empty() {
        return Err(UpdateError::InvalidManifest(
            "release_id and components are required".into(),
        ));
    }
    let mut ids = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for component in &manifest.components {
        if !ids.insert(component.id.as_str()) {
            return Err(UpdateError::InvalidManifest(format!(
                "duplicate component id {}",
                component.id
            )));
        }
        if !targets.insert(component.target.as_str()) {
            return Err(UpdateError::InvalidManifest(format!(
                "duplicate target {}",
                component.target
            )));
        }
        validate_relative_path(&component.target)?;
        if Path::new(&component.target)
            .file_name()
            .is_some_and(|name| {
                name.to_string_lossy()
                    .eq_ignore_ascii_case("ilia-updater.exe")
            })
        {
            return Err(UpdateError::InvalidManifest(
                "ilia-updater.exe must be replaced by a full installer".into(),
            ));
        }
        if component.payload_size == 0
            || component.payload_sha256.len() != 64
            || !component
                .payload_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(UpdateError::InvalidManifest(format!(
                "invalid payload metadata for {}",
                component.id
            )));
        }
        if !component.payload_url.starts_with("https://")
            && !component.payload_url.starts_with("file://")
        {
            return Err(UpdateError::InvalidManifest(format!(
                "payload URL for {} must use HTTPS",
                component.id
            )));
        }
        if component.kind == ComponentKind::Corpus
            && component.payload_format != PayloadFormat::SqlitePatch
        {
            return Err(UpdateError::InvalidManifest(
                "corpus updates must use sqlite_patch".into(),
            ));
        }
        if component.kind != ComponentKind::Corpus
            && component.payload_format == PayloadFormat::SqlitePatch
        {
            return Err(UpdateError::InvalidManifest(
                "only corpus updates may use sqlite_patch".into(),
            ));
        }
    }
    Ok(())
}

pub fn fetch_verified_release(
    manifest_url: &str,
    signature_url: &str,
    trusted_key: &TrustedPublicKey,
) -> Result<VerifiedRelease, UpdateError> {
    let manifest = read_url(manifest_url)?;
    let signature = read_url(signature_url)?;
    verify_manifest(&manifest, &signature, trusted_key)
}

pub struct UpdateEngine {
    install_root: PathBuf,
    work_root: PathBuf,
}

impl UpdateEngine {
    pub fn new(install_root: impl Into<PathBuf>) -> Result<Self, UpdateError> {
        let install_root = install_root.into();
        if !install_root.is_absolute() {
            return Err(UpdateError::UnsafeTarget(
                install_root.display().to_string(),
            ));
        }
        Ok(Self {
            work_root: install_root.join(".ilia-update"),
            install_root,
        })
    }

    pub fn stage(&self, release: &VerifiedRelease) -> Result<PathBuf, UpdateError> {
        let stage_root = self
            .work_root
            .join("staging")
            .join(safe_segment(&release.manifest.release_id)?);
        if stage_root.exists() {
            fs::remove_dir_all(&stage_root)?;
        }
        fs::create_dir_all(&stage_root)?;
        for component in &release.manifest.components {
            let destination = stage_root.join(format!("{}.payload", safe_segment(&component.id)?));
            download_payload(component, &destination)?;
        }
        let journal = UpdateJournal {
            release_id: release.manifest.release_id.clone(),
            status: JournalStatus::Staged,
            applied_components: Vec::new(),
            error: None,
        };
        self.write_journal(&journal)?;
        Ok(stage_root)
    }

    pub fn apply(
        &self,
        release: &VerifiedRelease,
        stage_root: &Path,
    ) -> Result<UpdateOutcome, UpdateError> {
        let mut versions = self.installed_versions()?;
        for component in &release.manifest.components {
            if let Some(expected) = &component.from_version {
                let actual = versions
                    .components
                    .get(&component.id)
                    .cloned()
                    .unwrap_or_else(|| "unversioned".into());
                if &actual != expected {
                    return Err(UpdateError::VersionMismatch {
                        component: component.id.clone(),
                        expected: expected.clone(),
                        actual,
                    });
                }
            }
        }

        let backup_root = self
            .work_root
            .join("backups")
            .join(safe_segment(&release.manifest.release_id)?);
        fs::create_dir_all(&backup_root)?;
        let mut journal = UpdateJournal {
            release_id: release.manifest.release_id.clone(),
            status: JournalStatus::Applying,
            applied_components: Vec::new(),
            error: None,
        };
        self.write_journal(&journal)?;

        for component in &release.manifest.components {
            let payload = stage_root.join(format!("{}.payload", safe_segment(&component.id)?));
            let result = self.apply_component(component, &payload, &backup_root);
            if let Err(error) = result {
                journal.status = JournalStatus::RollingBack;
                journal.error = Some(error.to_string());
                self.write_journal(&journal)?;
                let mut rollback_ids = journal.applied_components.clone();
                rollback_ids.push(component.id.clone());
                let rollback =
                    self.rollback_components(&release.manifest, &rollback_ids, &backup_root);
                journal.status = if rollback.is_empty() {
                    JournalStatus::RolledBack
                } else {
                    JournalStatus::Failed
                };
                self.write_journal(&journal)?;
                return Err(UpdateError::ApplyAndRollback {
                    apply: error.to_string(),
                    rollback,
                });
            }
            journal.applied_components.push(component.id.clone());
            self.write_journal(&journal)?;
        }

        for component in &release.manifest.components {
            versions
                .components
                .insert(component.id.clone(), component.version.clone());
        }
        self.write_installed_versions(&versions)?;
        journal.status = JournalStatus::Applied;
        self.write_journal(&journal)?;
        Ok(UpdateOutcome {
            release_id: release.manifest.release_id.clone(),
            applied_components: journal.applied_components,
            backup_root,
        })
    }

    pub fn rollback(&self, manifest: &UpdateManifest) -> Result<(), UpdateError> {
        let backup_root = self
            .work_root
            .join("backups")
            .join(safe_segment(&manifest.release_id)?);
        let applied = manifest
            .components
            .iter()
            .map(|component| component.id.clone())
            .collect::<Vec<_>>();
        let errors = self.rollback_components(manifest, &applied, &backup_root);
        if errors.is_empty() {
            let mut versions = self.installed_versions()?;
            for component in &manifest.components {
                if let Some(previous) = &component.from_version {
                    versions
                        .components
                        .insert(component.id.clone(), previous.clone());
                } else {
                    versions.components.remove(&component.id);
                }
            }
            self.write_installed_versions(&versions)?;
            Ok(())
        } else {
            Err(UpdateError::ApplyAndRollback {
                apply: "manual rollback".into(),
                rollback: errors,
            })
        }
    }

    pub fn installed_versions(&self) -> Result<InstalledVersions, UpdateError> {
        let path = self.work_root.join("versions.json");
        if !path.exists() {
            return Ok(InstalledVersions::default());
        }
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    fn apply_component(
        &self,
        component: &UpdateComponent,
        payload: &Path,
        backup_root: &Path,
    ) -> Result<(), UpdateError> {
        verify_payload_file(component, payload)?;
        let target = self.target_path(&component.target)?;
        let backup = backup_root.join(safe_segment(&component.id)?);
        let missing_marker = backup.with_extension("ilia-missing");
        if backup.exists() {
            remove_path(&backup)?;
        }
        if missing_marker.exists() {
            fs::remove_file(&missing_marker)?;
        }
        if let Some(parent) = backup.parent() {
            fs::create_dir_all(parent)?;
        }

        match component.payload_format {
            PayloadFormat::RawFile => {
                if target.exists() {
                    fs::rename(&target, &backup)?;
                } else {
                    fs::write(&missing_marker, b"target did not exist before update")?;
                }
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::rename(payload, &target)?;
            }
            PayloadFormat::SqlitePatch => {
                if !target.is_file() {
                    return Err(UpdateError::InvalidPayload {
                        component: component.id.clone(),
                        reason: format!("database is missing: {}", target.display()),
                    });
                }
                fs::copy(&target, &backup)?;
                let bytes = fs::read(payload)?;
                let patch: SqlitePatch = serde_json::from_slice(&bytes)?;
                if patch.schema_version != UPDATE_SCHEMA_VERSION || patch.statements.is_empty() {
                    return Err(UpdateError::InvalidPayload {
                        component: component.id.clone(),
                        reason: "invalid SQLite patch".into(),
                    });
                }
                let mut connection = Connection::open(&target)?;
                let transaction = connection.transaction()?;
                for statement in patch.statements {
                    transaction.execute_batch(&statement)?;
                }
                let integrity: String =
                    transaction.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
                if integrity != "ok" {
                    return Err(UpdateError::InvalidPayload {
                        component: component.id.clone(),
                        reason: format!("SQLite integrity_check returned {integrity}"),
                    });
                }
                transaction.commit()?;
            }
        }
        Ok(())
    }

    fn rollback_components(
        &self,
        manifest: &UpdateManifest,
        applied: &[String],
        backup_root: &Path,
    ) -> Vec<String> {
        let mut errors = Vec::new();
        for component_id in applied.iter().rev() {
            let Some(component) = manifest
                .components
                .iter()
                .find(|component| &component.id == component_id)
            else {
                continue;
            };
            let result = (|| -> Result<(), UpdateError> {
                let target = self.target_path(&component.target)?;
                let backup = backup_root.join(safe_segment(&component.id)?);
                let missing_marker = backup.with_extension("ilia-missing");
                if !backup.exists() && !missing_marker.exists() {
                    return Ok(());
                }
                remove_path(&target)?;
                if missing_marker.exists() {
                    fs::remove_file(missing_marker)?;
                } else if component.payload_format == PayloadFormat::SqlitePatch {
                    fs::copy(&backup, &target)?;
                } else {
                    fs::rename(&backup, &target)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                errors.push(format!("{component_id}: {error}"));
            }
        }
        errors
    }

    fn target_path(&self, relative: &str) -> Result<PathBuf, UpdateError> {
        validate_relative_path(relative)?;
        Ok(self.install_root.join(relative))
    }

    fn write_installed_versions(&self, versions: &InstalledVersions) -> Result<(), UpdateError> {
        fs::create_dir_all(&self.work_root)?;
        write_atomic(
            &self.work_root.join("versions.json"),
            &serde_json::to_vec_pretty(versions)?,
        )
    }

    fn write_journal(&self, journal: &UpdateJournal) -> Result<(), UpdateError> {
        fs::create_dir_all(&self.work_root)?;
        write_atomic(
            &self.work_root.join("journal.json"),
            &serde_json::to_vec_pretty(journal)?,
        )
    }
}

pub fn sha256_file(path: &Path) -> Result<String, UpdateError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn read_url(url: &str) -> Result<Vec<u8>, UpdateError> {
    if url.starts_with("file://") {
        let path = file_url_path(url)?;
        return Ok(fs::read(path)?);
    }
    Ok(ureq::get(url).call()?.body_mut().read_to_vec()?)
}

fn download_payload(component: &UpdateComponent, destination: &Path) -> Result<(), UpdateError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = destination.with_extension("ilia-new");
    remove_path(&temporary)?;
    let result = (|| -> Result<(), UpdateError> {
        let mut output = File::create(&temporary)?;
        if component.payload_url.starts_with("file://") {
            let mut input = File::open(file_url_path(&component.payload_url)?)?;
            stream_payload(component, &mut input, &mut output)?;
        } else {
            let mut response = ureq::get(&component.payload_url).call()?;
            let mut input = response.body_mut().as_reader();
            stream_payload(component, &mut input, &mut output)?;
        }
        output.sync_all()?;
        if destination.exists() {
            fs::remove_file(destination)?;
        }
        fs::rename(&temporary, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = remove_path(&temporary);
    }
    result
}

fn stream_payload(
    component: &UpdateComponent,
    input: &mut dyn Read,
    output: &mut File,
) -> Result<(), UpdateError> {
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        total += read as u64;
        if total > component.payload_size {
            return Err(payload_size_error(component, total));
        }
    }
    if total != component.payload_size {
        return Err(payload_size_error(component, total));
    }
    let actual = format!("{:x}", hasher.finalize());
    verify_payload_hash(component, &actual)
}

fn verify_payload_file(component: &UpdateComponent, path: &Path) -> Result<(), UpdateError> {
    let actual_size = fs::metadata(path)?.len();
    if actual_size != component.payload_size {
        return Err(payload_size_error(component, actual_size));
    }
    let actual = sha256_file(path)?;
    verify_payload_hash(component, &actual)
}

fn payload_size_error(component: &UpdateComponent, actual: u64) -> UpdateError {
    UpdateError::InvalidPayload {
        component: component.id.clone(),
        reason: format!(
            "size mismatch: expected {}, got {actual}",
            component.payload_size
        ),
    }
}

fn verify_payload_hash(component: &UpdateComponent, actual: &str) -> Result<(), UpdateError> {
    if actual != component.payload_sha256.to_ascii_lowercase() {
        return Err(UpdateError::InvalidPayload {
            component: component.id.clone(),
            reason: format!(
                "SHA-256 mismatch: expected {}, got {actual}",
                component.payload_sha256
            ),
        });
    }
    Ok(())
}

fn file_url_path(url: &str) -> Result<PathBuf, UpdateError> {
    url::Url::parse(url)
        .map_err(|error| UpdateError::InvalidManifest(error.to_string()))?
        .to_file_path()
        .map_err(|_| UpdateError::InvalidManifest(format!("invalid file URL: {url}")))
}

fn validate_relative_path(path: &str) -> Result<(), UpdateError> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(UpdateError::UnsafeTarget(path.display().to_string()));
    }
    Ok(())
}

fn safe_segment(value: &str) -> Result<&str, UpdateError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '-' | '_' | '.')
        })
    {
        return Err(UpdateError::InvalidManifest(format!(
            "unsafe identifier {value}"
        )));
    }
    Ok(value)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), UpdateError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("ilia-new");
    fs::write(&temporary, bytes)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn remove_path(path: &Path) -> Result<(), UpdateError> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ilia-updater-{label}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn component(
        id: &str,
        kind: ComponentKind,
        target: &str,
        format: PayloadFormat,
        payload: &[u8],
    ) -> UpdateComponent {
        UpdateComponent {
            id: id.into(),
            kind,
            version: "2.0.0".into(),
            from_version: None,
            target: target.into(),
            payload_url: "file:///payload".into(),
            payload_size: payload.len() as u64,
            payload_sha256: format!("{:x}", Sha256::digest(payload)),
            payload_format: format,
        }
    }

    #[test]
    fn verifies_ed25519_detached_signature() {
        let random = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&random).unwrap();
        let signing = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let manifest = UpdateManifest {
            schema_version: 1,
            release_id: "release-1".into(),
            channel: "stable".into(),
            created_at: "2026-09-23T00:00:00Z".into(),
            components: vec![component(
                "application",
                ComponentKind::Application,
                "ilia-desktop.exe",
                PayloadFormat::RawFile,
                b"new app",
            )],
        };
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let detached = DetachedSignature {
            key_id: "test-key".into(),
            algorithm: "ed25519".into(),
            signature_base64: STANDARD.encode(signing.sign(&bytes).as_ref()),
        };
        let trusted = TrustedPublicKey {
            key_id: "test-key".into(),
            algorithm: "ed25519".into(),
            public_key_base64: STANDARD.encode(signing.public_key().as_ref()),
        };
        verify_manifest(&bytes, &serde_json::to_vec(&detached).unwrap(), &trusted).unwrap();
        let mut tampered = bytes;
        tampered.push(b' ');
        assert!(matches!(
            verify_manifest(&tampered, &serde_json::to_vec(&detached).unwrap(), &trusted),
            Err(UpdateError::InvalidSignature)
        ));
    }

    #[test]
    fn applies_raw_file_and_rolls_back_after_later_failure() {
        let root = temp_root("rollback");
        fs::write(root.join("app.exe"), b"old app").unwrap();
        let stage = root.join("stage");
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("application.payload"), b"new app").unwrap();
        fs::write(stage.join("broken.payload"), b"wrong").unwrap();
        let manifest = UpdateManifest {
            schema_version: 1,
            release_id: "rollback-release".into(),
            channel: "stable".into(),
            created_at: "2026-09-23T00:00:00Z".into(),
            components: vec![
                component(
                    "application",
                    ComponentKind::Application,
                    "app.exe",
                    PayloadFormat::RawFile,
                    b"new app",
                ),
                component(
                    "broken",
                    ComponentKind::Model,
                    "models/broken.bin",
                    PayloadFormat::RawFile,
                    b"expected",
                ),
            ],
        };
        let release = VerifiedRelease {
            manifest,
            manifest_bytes: Vec::new(),
        };
        let engine = UpdateEngine::new(root.clone()).unwrap();
        assert!(engine.apply(&release, &stage).is_err());
        assert_eq!(fs::read(root.join("app.exe")).unwrap(), b"old app");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn applies_sqlite_patch_transactionally() {
        let root = temp_root("sqlite");
        let database = root.join("data.sqlite3");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch("CREATE TABLE items(id INTEGER PRIMARY KEY, value TEXT);")
            .unwrap();
        drop(connection);
        let patch = serde_json::to_vec(&SqlitePatch {
            schema_version: 1,
            statements: vec!["INSERT INTO items(value) VALUES ('updated');".into()],
        })
        .unwrap();
        let stage = root.join("stage");
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("corpus.payload"), &patch).unwrap();
        let manifest = UpdateManifest {
            schema_version: 1,
            release_id: "corpus-release".into(),
            channel: "stable".into(),
            created_at: "2026-09-23T00:00:00Z".into(),
            components: vec![component(
                "corpus",
                ComponentKind::Corpus,
                "data.sqlite3",
                PayloadFormat::SqlitePatch,
                &patch,
            )],
        };
        let release = VerifiedRelease {
            manifest,
            manifest_bytes: Vec::new(),
        };
        UpdateEngine::new(root.clone())
            .unwrap()
            .apply(&release, &stage)
            .unwrap();
        let connection = Connection::open(&database).unwrap();
        let count: i64 = connection
            .query_row("SELECT count(*) FROM items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stages_large_payload_without_buffering_the_whole_file() {
        let root = temp_root("stream");
        let source = root.join("model.gguf");
        let payload = vec![0x5a; 12 * 1024 * 1024];
        fs::write(&source, &payload).unwrap();
        let mut model = component(
            "model",
            ComponentKind::Model,
            "models/model.gguf",
            PayloadFormat::RawFile,
            &payload,
        );
        model.payload_url = url::Url::from_file_path(&source).unwrap().to_string();
        let release = VerifiedRelease {
            manifest: UpdateManifest {
                schema_version: 1,
                release_id: "stream-release".into(),
                channel: "stable".into(),
                created_at: "2026-09-23T00:00:00Z".into(),
                components: vec![model],
            },
            manifest_bytes: Vec::new(),
        };
        let staged = UpdateEngine::new(root.clone())
            .unwrap()
            .stage(&release)
            .unwrap();
        assert_eq!(
            sha256_file(&staged.join("model.payload")).unwrap(),
            format!("{:x}", Sha256::digest(&payload))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_updater_self_replacement() {
        let manifest = UpdateManifest {
            schema_version: 1,
            release_id: "unsafe-self-update".into(),
            channel: "stable".into(),
            created_at: "2026-09-23T00:00:00Z".into(),
            components: vec![component(
                "updater",
                ComponentKind::Application,
                "bin/ilia-updater.exe",
                PayloadFormat::RawFile,
                b"new updater",
            )],
        };
        assert!(matches!(
            validate_manifest(&manifest),
            Err(UpdateError::InvalidManifest(_))
        ));
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(validate_relative_path("../outside").is_err());
        assert!(validate_relative_path("C:\\outside").is_err());
    }
}
