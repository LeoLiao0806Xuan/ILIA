use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ring::signature::{ED25519, UnparsedPublicKey};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const UPDATE_SCHEMA_VERSION: u32 = 1;
pub const LOCAL_PACKAGE_VERSION: u32 = 1;
pub const MAX_LOCAL_PACKAGE_BYTES: u64 = 16 * 1024 * 1024 * 1024;

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
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),
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
    #[error("invalid local update package: {0}")]
    InvalidPackage(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateDiagnostic {
    pub code: String,
    pub message_zh: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyConfig {
    pub url: String,
}

impl std::fmt::Debug for ProxyConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProxyConfig")
            .field("url", &redact_url(&self.url))
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalPackageManifest {
    pub format_version: u32,
    pub release_id: String,
    pub payloads: Vec<LocalPackagePayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalPackagePayload {
    pub component_id: String,
    pub path: String,
    pub size: u64,
    pub sha256: String,
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

pub fn fetch_verified_release_with_proxy(
    manifest_url: &str,
    signature_url: &str,
    trusted_key: &TrustedPublicKey,
    proxy: Option<&ProxyConfig>,
) -> Result<VerifiedRelease, UpdateError> {
    let manifest = read_url_with_proxy(manifest_url, proxy)?;
    let signature = read_url_with_proxy(signature_url, proxy)?;
    verify_manifest(&manifest, &signature, trusted_key)
}

pub fn diagnose_update_error(error: &UpdateError) -> UpdateDiagnostic {
    let (code, message) = match error {
        UpdateError::InvalidSignature => ("invalid_signature", "更新签名无效，已拒绝安装。"),
        UpdateError::InvalidPayload { .. } | UpdateError::InvalidPackage(_) => {
            ("invalid_payload", "更新文件损坏或格式不受信任。")
        }
        UpdateError::Http(ureq::Error::HostNotFound) => {
            ("dns_failed", "无法解析更新服务器域名，请检查 DNS。")
        }
        UpdateError::Http(ureq::Error::Timeout(_)) => ("connect_timeout", "连接更新服务器超时。"),
        UpdateError::Http(ureq::Error::Io(io))
            if matches!(
                io.kind(),
                io::ErrorKind::UnexpectedEof
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::BrokenPipe
            ) =>
        {
            ("download_interrupted", "更新下载被中断，可稍后重试。")
        }
        UpdateError::Http(_) => (
            "server_unreachable",
            "无法访问更新服务器，离线研究仍可继续使用。",
        ),
        _ => ("invalid_payload", "更新未能通过本地验证。"),
    };
    UpdateDiagnostic {
        code: code.into(),
        message_zh: message.into(),
    }
}

pub fn redact_url(value: &str) -> String {
    match url::Url::parse(value) {
        Ok(mut parsed) => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.to_string()
        }
        Err(_) => "<invalid-or-redacted-proxy-url>".into(),
    }
}

pub fn create_local_package(
    output: &Path,
    manifest_path: &Path,
    signature_path: &Path,
) -> Result<(), UpdateError> {
    let manifest_bytes = fs::read(manifest_path)?;
    let manifest: UpdateManifest = serde_json::from_slice(&manifest_bytes)?;
    validate_manifest(&manifest)?;
    let signature = fs::read(signature_path)?;
    let mut payloads = Vec::new();
    for component in &manifest.components {
        let source = file_url_path(&component.payload_url)?;
        verify_payload_file(component, &source)?;
        payloads.push((
            LocalPackagePayload {
                component_id: component.id.clone(),
                path: format!("payloads/{}.payload", safe_segment(&component.id)?),
                size: component.payload_size,
                sha256: component.payload_sha256.clone(),
            },
            source,
        ));
    }
    let package = LocalPackageManifest {
        format_version: LOCAL_PACKAGE_VERSION,
        release_id: manifest.release_id.clone(),
        payloads: payloads.iter().map(|(entry, _)| entry.clone()).collect(),
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = output.with_extension("ilia-new");
    let file = File::create(&temporary)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o600);
    zip.start_file("package.json", options)?;
    zip.write_all(&serde_json::to_vec_pretty(&package)?)?;
    zip.start_file("update-manifest.json", options)?;
    zip.write_all(&manifest_bytes)?;
    zip.start_file("update-manifest.sig", options)?;
    zip.write_all(&signature)?;
    for (entry, source) in payloads {
        zip.start_file(entry.path, options)?;
        let mut input = File::open(source)?;
        io::copy(&mut input, &mut zip)?;
    }
    zip.finish()?;
    if output.exists() {
        fs::remove_file(output)?;
    }
    fs::rename(temporary, output)?;
    Ok(())
}

pub fn load_local_package(
    package_path: &Path,
    trusted_key: &TrustedPublicKey,
    extraction_root: &Path,
) -> Result<(VerifiedRelease, PathBuf), UpdateError> {
    if fs::metadata(package_path)?.len() > MAX_LOCAL_PACKAGE_BYTES {
        return Err(UpdateError::InvalidPackage(
            "package exceeds size limit".into(),
        ));
    }
    let mut archive = ZipArchive::new(File::open(package_path)?)
        .map_err(|e| UpdateError::InvalidPackage(e.to_string()))?;
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|e| UpdateError::InvalidPackage(e.to_string()))?;
        let name = file.name().to_owned();
        if !names.insert(name.clone()) {
            return Err(UpdateError::InvalidPackage(format!(
                "duplicate entry {name}"
            )));
        }
        if file.is_dir()
            || file.enclosed_name().is_none()
            || file
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(UpdateError::InvalidPackage(format!("unsafe entry {name}")));
        }
        total = total
            .checked_add(file.size())
            .ok_or_else(|| UpdateError::InvalidPackage("declared size overflow".into()))?;
        if total > MAX_LOCAL_PACKAGE_BYTES {
            return Err(UpdateError::InvalidPackage(
                "expanded package exceeds size limit".into(),
            ));
        }
    }
    let package: LocalPackageManifest = read_zip_json(&mut archive, "package.json")?;
    if package.format_version != LOCAL_PACKAGE_VERSION {
        return Err(UpdateError::InvalidPackage(format!(
            "unsupported format {}",
            package.format_version
        )));
    }
    let manifest_bytes = read_zip_bytes(&mut archive, "update-manifest.json", 4 * 1024 * 1024)?;
    let signature_bytes = read_zip_bytes(&mut archive, "update-manifest.sig", 64 * 1024)?;
    let mut release = verify_manifest(&manifest_bytes, &signature_bytes, trusted_key)?;
    if release.manifest.release_id != package.release_id {
        return Err(UpdateError::InvalidPackage("release id mismatch".into()));
    }
    if extraction_root.exists() {
        remove_path(extraction_root)?;
    }
    fs::create_dir_all(extraction_root)?;
    let mut expected = BTreeSet::new();
    for entry in &package.payloads {
        validate_relative_path(&entry.path)?;
        if !expected.insert(entry.component_id.clone()) {
            return Err(UpdateError::InvalidPackage(
                "duplicate component metadata".into(),
            ));
        }
        let component = release
            .manifest
            .components
            .iter_mut()
            .find(|v| v.id == entry.component_id)
            .ok_or_else(|| {
                UpdateError::InvalidPackage(format!("unknown component {}", entry.component_id))
            })?;
        if entry.size != component.payload_size
            || !entry.sha256.eq_ignore_ascii_case(&component.payload_sha256)
        {
            return Err(UpdateError::InvalidPackage(format!(
                "metadata mismatch for {}",
                entry.component_id
            )));
        }
        let expected_path = format!("payloads/{}.payload", safe_segment(&entry.component_id)?);
        if entry.path != expected_path {
            return Err(UpdateError::InvalidPackage(
                "non-canonical payload path".into(),
            ));
        }
        let bytes = read_zip_bytes(&mut archive, &entry.path, component.payload_size)?;
        let destination =
            extraction_root.join(format!("{}.payload", safe_segment(&entry.component_id)?));
        fs::write(&destination, &bytes)?;
        verify_payload_file(component, &destination)?;
        component.payload_url = url::Url::from_file_path(&destination)
            .map_err(|_| UpdateError::InvalidPackage("cannot form payload URL".into()))?
            .to_string();
    }
    if expected.len() != release.manifest.components.len() {
        return Err(UpdateError::InvalidPackage(
            "payload list incomplete".into(),
        ));
    }
    Ok((release, extraction_root.to_path_buf()))
}

fn read_zip_json<T: serde::de::DeserializeOwned>(
    archive: &mut ZipArchive<File>,
    name: &str,
) -> Result<T, UpdateError> {
    Ok(serde_json::from_slice(&read_zip_bytes(
        archive,
        name,
        4 * 1024 * 1024,
    )?)?)
}
fn read_zip_bytes(
    archive: &mut ZipArchive<File>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, UpdateError> {
    let mut file = archive
        .by_name(name)
        .map_err(|_| UpdateError::InvalidPackage(format!("missing {name}")))?;
    if file.size() > limit {
        return Err(UpdateError::InvalidPackage(format!(
            "{name} exceeds declared limit"
        )));
    }
    let mut bytes = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
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
        let install_root = fs::canonicalize(install_root)?;
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
                let target = self.target_path(&component.target)?;
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
        let target = self.install_root.join(relative);
        ensure_no_reparse_points(&self.install_root, &target)?;
        Ok(target)
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
    read_url_with_proxy(url, None)
}

fn read_url_with_proxy(url: &str, proxy: Option<&ProxyConfig>) -> Result<Vec<u8>, UpdateError> {
    if url.starts_with("file://") {
        let path = file_url_path(url)?;
        return Ok(fs::read(path)?);
    }
    let mut builder =
        ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(30)));
    if let Some(proxy) = proxy {
        builder = builder.proxy(Some(ureq::Proxy::new(&proxy.url)?));
    }
    let agent: ureq::Agent = builder.build().into();
    Ok(agent.get(url).call()?.body_mut().read_to_vec()?)
}

fn download_payload(component: &UpdateComponent, destination: &Path) -> Result<(), UpdateError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = destination.with_extension("part");
    let state_path = destination.with_extension("part.json");
    let mut offset = resumable_offset(component, &temporary, &state_path)?;
    let result = (|| -> Result<(), UpdateError> {
        let mut output = OpenOptions::new()
            .create(true)
            .append(offset > 0)
            .write(true)
            .truncate(offset == 0)
            .open(&temporary)?;
        let mut hasher = Sha256::new();
        if offset > 0 {
            let mut existing = File::open(&temporary)?;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = existing.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
        }
        if component.payload_url.starts_with("file://") {
            let mut input = File::open(file_url_path(&component.payload_url)?)?;
            use std::io::{Seek, SeekFrom};
            input.seek(SeekFrom::Start(offset))?;
            stream_payload_from(component, &mut input, &mut output, offset, hasher)?;
        } else {
            let mut request = ureq::get(&component.payload_url);
            if offset > 0 {
                request = request.header("Range", &format!("bytes={offset}-"));
            }
            let mut response = request.call()?;
            if offset > 0 && response.status() != 206 {
                output = File::create(&temporary)?;
                offset = 0;
                hasher = Sha256::new();
            }
            let mut input = response.body_mut().as_reader();
            stream_payload_from(component, &mut input, &mut output, offset, hasher)?;
        }
        output.sync_all()?;
        if destination.exists() {
            fs::remove_file(destination)?;
        }
        fs::rename(&temporary, destination)?;
        remove_path(&state_path)?;
        Ok(())
    })();
    if matches!(&result, Err(UpdateError::InvalidPayload { .. })) {
        let _ = remove_path(&temporary);
        let _ = remove_path(&state_path);
    } else if result.is_err() && temporary.is_file() {
        let current = fs::metadata(&temporary)
            .map(|v| v.len())
            .unwrap_or(0)
            .min(component.payload_size);
        let state = PartialDownload {
            component_id: component.id.clone(),
            payload_sha256: component.payload_sha256.clone(),
            payload_size: component.payload_size,
            offset: current,
        };
        let _ = write_atomic(&state_path, &serde_json::to_vec(&state).unwrap_or_default());
    }
    result
}

#[derive(Serialize, Deserialize)]
struct PartialDownload {
    component_id: String,
    payload_sha256: String,
    payload_size: u64,
    offset: u64,
}

fn resumable_offset(
    component: &UpdateComponent,
    partial: &Path,
    state: &Path,
) -> Result<u64, UpdateError> {
    if !partial.is_file() || !state.is_file() {
        return Ok(0);
    }
    let parsed: PartialDownload = match serde_json::from_slice(&fs::read(state)?) {
        Ok(v) => v,
        Err(_) => {
            remove_path(partial)?;
            remove_path(state)?;
            return Ok(0);
        }
    };
    let actual = fs::metadata(partial)?.len();
    if parsed.component_id != component.id
        || parsed.payload_sha256 != component.payload_sha256
        || parsed.payload_size != component.payload_size
        || parsed.offset != actual
        || actual >= component.payload_size
    {
        remove_path(partial)?;
        remove_path(state)?;
        return Ok(0);
    }
    Ok(actual)
}

fn stream_payload_from(
    component: &UpdateComponent,
    input: &mut dyn Read,
    output: &mut File,
    mut total: u64,
    mut hasher: Sha256,
) -> Result<(), UpdateError> {
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
        return Err(UpdateError::Io(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!(
                "download interrupted at {total} of {} bytes",
                component.payload_size
            ),
        )));
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

fn ensure_no_reparse_points(root: &Path, target: &Path) -> Result<(), UpdateError> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| UpdateError::UnsafeTarget(target.display().to_string()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata_is_reparse_or_symlink(&metadata) => {
                return Err(UpdateError::UnsafeTarget(current.display().to_string()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn metadata_is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
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
    fn resumes_a_matching_partial_file_payload() {
        let root = temp_root("resume");
        let source = root.join("source.bin");
        let payload = vec![0x31; 2 * 1024 * 1024];
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
                release_id: "resume-release".into(),
                channel: "stable".into(),
                created_at: "2026-09-26T00:00:00Z".into(),
                components: vec![model.clone()],
            },
            manifest_bytes: vec![],
        };
        let engine = UpdateEngine::new(root.clone()).unwrap();
        let stage = root.join(".ilia-update/staging/resume-release");
        fs::create_dir_all(&stage).unwrap();
        let partial = stage.join("model.part");
        fs::write(&partial, &payload[..512 * 1024]).unwrap();
        let state = PartialDownload {
            component_id: model.id,
            payload_sha256: model.payload_sha256,
            payload_size: model.payload_size,
            offset: 512 * 1024,
        };
        fs::write(
            stage.join("model.part.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        let staged = engine.stage(&release).unwrap();
        assert_eq!(fs::read(staged.join("model.payload")).unwrap(), payload);
        assert!(!staged.join("model.part.json").exists());
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
    fn rejects_reparse_or_symlink_target_parent() {
        let root = temp_root("reparse");
        let real = root.join("real-models");
        let linked = root.join("models");
        fs::create_dir_all(&real).unwrap();
        if create_directory_symlink(&real, &linked).is_err() {
            fs::remove_dir_all(root).unwrap();
            return;
        }
        let engine = UpdateEngine::new(root.clone()).unwrap();
        assert!(matches!(
            engine.target_path("models/model.gguf"),
            Err(UpdateError::UnsafeTarget(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(validate_relative_path("../outside").is_err());
        assert!(validate_relative_path("C:\\outside").is_err());
    }

    #[test]
    fn creates_and_loads_signed_local_package() {
        let root = temp_root("local-package");
        let payload = root.join("app.payload");
        fs::write(&payload, b"new app").unwrap();
        let mut app = component(
            "application",
            ComponentKind::Application,
            "app.exe",
            PayloadFormat::RawFile,
            b"new app",
        );
        app.payload_url = url::Url::from_file_path(&payload).unwrap().to_string();
        let manifest = UpdateManifest {
            schema_version: 1,
            release_id: "local-1".into(),
            channel: "stable".into(),
            created_at: "2026-09-26T00:00:00Z".into(),
            components: vec![app],
        };
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        let random = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&random).unwrap();
        let signing = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let signature = serde_json::to_vec(&DetachedSignature {
            key_id: "local-test".into(),
            algorithm: "ed25519".into(),
            signature_base64: STANDARD.encode(signing.sign(&manifest_bytes).as_ref()),
        })
        .unwrap();
        let trusted = TrustedPublicKey {
            key_id: "local-test".into(),
            algorithm: "ed25519".into(),
            public_key_base64: STANDARD.encode(signing.public_key().as_ref()),
        };
        let manifest_path = root.join("update-manifest.json");
        let signature_path = root.join("update-manifest.sig");
        fs::write(&manifest_path, &manifest_bytes).unwrap();
        fs::write(&signature_path, &signature).unwrap();
        let package = root.join("release.ilia");
        create_local_package(&package, &manifest_path, &signature_path).unwrap();
        let (release, stage) = load_local_package(&package, &trusted, &root.join("stage")).unwrap();
        assert_eq!(release.manifest.release_id, "local-1");
        assert_eq!(
            fs::read(stage.join("application.payload")).unwrap(),
            b"new app"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_package_rejects_path_traversal_entries() {
        let root = temp_root("evil-package");
        let package = root.join("evil.ilia");
        let file = File::create(&package).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.start_file("../package.json", options).unwrap();
        zip.write_all(b"{}").unwrap();
        zip.finish().unwrap();
        let trusted = TrustedPublicKey {
            key_id: "x".into(),
            algorithm: "ed25519".into(),
            public_key_base64: STANDARD.encode([0_u8; 32]),
        };
        assert!(matches!(
            load_local_package(&package, &trusted, &root.join("stage")),
            Err(UpdateError::InvalidPackage(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn update_diagnostics_are_stable_and_proxy_debug_is_redacted() {
        assert_eq!(
            diagnose_update_error(&UpdateError::InvalidSignature).code,
            "invalid_signature"
        );
        let proxy = ProxyConfig {
            url: "socks5://alice:secret@127.0.0.1:1080".into(),
        };
        let debug = format!("{proxy:?}");
        assert!(!debug.contains("alice"));
        assert!(!debug.contains("secret"));
    }

    #[cfg(windows)]
    fn create_directory_symlink(original: &Path, link: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_dir(original, link)
    }

    #[cfg(unix)]
    fn create_directory_symlink(original: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(original, link)
    }
}
