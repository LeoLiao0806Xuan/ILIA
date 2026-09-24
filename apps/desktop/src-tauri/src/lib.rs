use std::{
    net::TcpListener,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use ilia_core::{AnswerResponse, DocumentSummary, SearchOptions, SearchResponse};
use ilia_database::Database;
use ilia_embedding::{BGE_M3_MODEL_ID, BgeM3Embedder};
use ilia_inference::{
    AnswerService, AutoManagedLlamaServer, RuntimeManager, RuntimeManagerConfig, RuntimePreference,
    RuntimeProbeReport, RuntimeStartupReport, TranslationResponse,
};
use ilia_retrieval::RetrievalService;
use ilia_updater::{
    InstalledVersions, TrustedPublicKey, UpdateEngine, UpdateManifest, fetch_verified_release,
};
use serde::Serialize;
use tauri::{Manager, State};

#[derive(Debug, Clone)]
struct AppPaths {
    install_root: PathBuf,
    database: PathBuf,
    bge_cache: PathBuf,
    qwen_model: PathBuf,
    runtime_root: PathBuf,
    onnx_runtime: PathBuf,
    log_dir: PathBuf,
    updater: PathBuf,
    trusted_update_key: PathBuf,
    normalized_corpus: PathBuf,
}

impl AppPaths {
    fn discover(app: &tauri::AppHandle) -> Result<Self, String> {
        let development_root = std::env::var_os("ILIA_ROOT").map(PathBuf::from);
        let root = development_root.clone().map(Ok).unwrap_or_else(|| {
            app.path()
                .resource_dir()
                .map_err(|error| format!("cannot resolve ILIA resource directory: {error}"))
        })?;
        let log_dir = app
            .path()
            .app_log_dir()
            .map_err(|error| format!("cannot resolve ILIA log directory: {error}"))?;
        std::fs::create_dir_all(&log_dir)
            .map_err(|error| format!("cannot create ILIA log directory: {error}"))?;
        let paths = Self {
            install_root: root.clone(),
            database: root.join("data/ilia.sqlite3"),
            bge_cache: root.join("models/bge-m3"),
            qwen_model: root.join("models/qwen3-4b/Qwen3-4B-Q4_K_M.gguf"),
            runtime_root: root.join("runtime"),
            onnx_runtime: root.join("runtime/onnx/onnxruntime.dll"),
            log_dir,
            updater: development_root
                .map(|root| root.join("target/x86_64-pc-windows-gnu/release/ilia-updater.exe"))
                .unwrap_or_else(|| root.join("ilia-updater.exe")),
            trusted_update_key: root.join("update/trusted-key.json"),
            normalized_corpus: root.join("corpus/normalized"),
        };
        for (label, path) in [
            ("database", &paths.database),
            ("BGE-M3 model", &paths.bge_cache),
            ("Qwen model", &paths.qwen_model),
            ("llama.cpp runtimes", &paths.runtime_root),
            ("ONNX Runtime", &paths.onnx_runtime),
            ("trusted update key", &paths.trusted_update_key),
            ("normalized legal-text library", &paths.normalized_corpus),
        ] {
            if !path.exists() {
                return Err(format!("{label} is missing: {}", path.display()));
            }
        }
        Ok(paths)
    }
}

struct RunningRuntime {
    server: AutoManagedLlamaServer,
    api_key: String,
}

struct DesktopServices {
    paths: AppPaths,
    embedder: Mutex<Option<BgeM3Embedder>>,
    runtime: Mutex<Option<RunningRuntime>>,
    preference: Mutex<RuntimePreference>,
}

#[derive(Serialize)]
struct DesktopAskResponse {
    runtime: RuntimeStartupReport,
    search: SearchResponse,
    answer: AnswerResponse,
}

#[derive(Serialize)]
struct DesktopUpdateStatus {
    manifest: UpdateManifest,
    installed_versions: InstalledVersions,
}

#[derive(Serialize)]
struct DesktopTranslationResponse {
    runtime: RuntimeStartupReport,
    translation: TranslationResponse,
}

impl DesktopServices {
    fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            embedder: Mutex::new(None),
            runtime: Mutex::new(None),
            preference: Mutex::new(RuntimePreference::Auto),
        }
    }

    fn search(&self, query: &str) -> Result<SearchResponse, String> {
        let query = query.trim();
        if query.is_empty() {
            return Err("问题不能为空".to_owned());
        }
        let database =
            Database::open_read_only(&self.paths.database).map_err(|error| error.to_string())?;
        let retrieval = RetrievalService::new(database);
        let mut embedder_guard = self
            .embedder
            .lock()
            .map_err(|_| "BGE-M3 state lock is poisoned".to_owned())?;
        if embedder_guard.is_none() {
            *embedder_guard = Some(
                BgeM3Embedder::new(self.paths.bge_cache.clone(), false)
                    .map_err(|error| error.to_string())?,
            );
        }
        let vector = embedder_guard
            .as_mut()
            .expect("embedder was initialized")
            .embed_query(query)
            .map_err(|error| error.to_string())?;
        drop(embedder_guard);
        retrieval
            .search_hybrid(query, &vector, BGE_M3_MODEL_ID, SearchOptions::default())
            .map_err(|error| error.to_string())
    }

    fn runtime_status(&self) -> Result<RuntimeProbeReport, String> {
        let preference = *self
            .preference
            .lock()
            .map_err(|_| "runtime preference lock is poisoned".to_owned())?;
        Ok(RuntimeManager::probe(&self.paths.runtime_root, preference))
    }

    fn set_preference(&self, preference: RuntimePreference) -> Result<RuntimeProbeReport, String> {
        *self
            .preference
            .lock()
            .map_err(|_| "runtime preference lock is poisoned".to_owned())? = preference;
        self.runtime
            .lock()
            .map_err(|_| "runtime state lock is poisoned".to_owned())?
            .take();
        self.runtime_status()
    }

    fn ask(&self, query: &str) -> Result<DesktopAskResponse, String> {
        let search = self.search(query)?;
        let preference = *self
            .preference
            .lock()
            .map_err(|_| "runtime preference lock is poisoned".to_owned())?;
        let mut runtime_guard = self
            .runtime
            .lock()
            .map_err(|_| "runtime state lock is poisoned".to_owned())?;
        if runtime_guard.is_none() {
            let api_key = uuid::Uuid::new_v4().simple().to_string();
            let server = RuntimeManager::start(&RuntimeManagerConfig {
                runtime_root: self.paths.runtime_root.clone(),
                model: self.paths.qwen_model.clone(),
                preference,
                host: "127.0.0.1".to_owned(),
                port: available_loopback_port().map_err(|error| error.to_string())?,
                startup_timeout: Duration::from_secs(120),
                log_dir: self.paths.log_dir.clone(),
                api_key: Some(api_key.clone()),
            })
            .map_err(|error| error.to_string())?;
            *runtime_guard = Some(RunningRuntime { server, api_key });
        }
        let running = runtime_guard.as_ref().expect("runtime was initialized");
        let runtime_report = running.server.report().clone();
        let answer = AnswerService::new(running.server.base_url())
            .with_api_key(&running.api_key)
            .answer(query, &search.evidence)
            .map_err(|error| error.to_string())?;
        Ok(DesktopAskResponse {
            runtime: runtime_report,
            search,
            answer,
        })
    }

    fn translate(
        &self,
        source_text: &str,
        citation_label: &str,
    ) -> Result<DesktopTranslationResponse, String> {
        let preference = *self
            .preference
            .lock()
            .map_err(|_| "runtime preference lock is poisoned".to_owned())?;
        let mut runtime_guard = self
            .runtime
            .lock()
            .map_err(|_| "runtime state lock is poisoned".to_owned())?;
        if runtime_guard.is_none() {
            let api_key = uuid::Uuid::new_v4().simple().to_string();
            let server = RuntimeManager::start(&RuntimeManagerConfig {
                runtime_root: self.paths.runtime_root.clone(),
                model: self.paths.qwen_model.clone(),
                preference,
                host: "127.0.0.1".to_owned(),
                port: available_loopback_port().map_err(|error| error.to_string())?,
                startup_timeout: Duration::from_secs(120),
                log_dir: self.paths.log_dir.clone(),
                api_key: Some(api_key.clone()),
            })
            .map_err(|error| error.to_string())?;
            *runtime_guard = Some(RunningRuntime { server, api_key });
        }
        let running = runtime_guard.as_ref().expect("runtime was initialized");
        let runtime_report = running.server.report().clone();
        let translation = AnswerService::new(running.server.base_url())
            .with_api_key(&running.api_key)
            .translate_to_simplified_chinese(source_text, citation_label)
            .map_err(|error| error.to_string())?;
        Ok(DesktopTranslationResponse {
            runtime: runtime_report,
            translation,
        })
    }

    fn documents(&self) -> Result<Vec<DocumentSummary>, String> {
        Database::open_read_only(&self.paths.database)
            .map_err(|error| error.to_string())?
            .documents()
            .map_err(|error| error.to_string())
    }

    fn read_document_text(&self, document_id: &str) -> Result<String, String> {
        if document_id.is_empty() || document_id.contains(['/', '\\']) || document_id.contains("..")
        {
            return Err("invalid document identifier".to_owned());
        }
        let database =
            Database::open_read_only(&self.paths.database).map_err(|error| error.to_string())?;
        if database
            .document(document_id)
            .map_err(|error| error.to_string())?
            .is_none()
        {
            return Err("document does not exist in the local library".to_owned());
        }
        let text = self
            .paths
            .normalized_corpus
            .join(document_id)
            .join("source.txt");
        if text.is_file() {
            let metadata = std::fs::metadata(&text).map_err(|error| error.to_string())?;
            if metadata.len() > 16 * 1024 * 1024 {
                return Err("local normalized text exceeds the 16 MiB display limit".to_owned());
            }
            return std::fs::read_to_string(&text).map_err(|error| error.to_string());
        }
        database
            .normalized_document_text(document_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "local normalized text is missing".to_owned())
    }
}

fn available_loopback_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

#[tauri::command]
async fn get_runtime_status(
    state: State<'_, Arc<DesktopServices>>,
) -> Result<RuntimeProbeReport, String> {
    let services = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || services.runtime_status())
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn set_runtime_preference(
    state: State<'_, Arc<DesktopServices>>,
    preference: String,
) -> Result<RuntimeProbeReport, String> {
    let preference = preference.parse::<RuntimePreference>()?;
    let services = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || services.set_preference(preference))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn search_documents(
    state: State<'_, Arc<DesktopServices>>,
    query: String,
) -> Result<SearchResponse, String> {
    let services = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || services.search(&query))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn ask_question(
    state: State<'_, Arc<DesktopServices>>,
    query: String,
) -> Result<DesktopAskResponse, String> {
    let services = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || services.ask(&query))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn translate_source(
    state: State<'_, Arc<DesktopServices>>,
    source_text: String,
    citation_label: String,
) -> Result<DesktopTranslationResponse, String> {
    let services = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || services.translate(&source_text, &citation_label))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn list_documents(
    state: State<'_, Arc<DesktopServices>>,
) -> Result<Vec<DocumentSummary>, String> {
    let services = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || services.documents())
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn read_document_text(
    state: State<'_, Arc<DesktopServices>>,
    document_id: String,
) -> Result<String, String> {
    let services = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || services.read_document_text(&document_id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn check_updates(
    state: State<'_, Arc<DesktopServices>>,
    manifest_url: String,
    signature_url: String,
) -> Result<DesktopUpdateStatus, String> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let trusted: TrustedPublicKey = serde_json::from_slice(
            &std::fs::read(&paths.trusted_update_key).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let release = fetch_verified_release(&manifest_url, &signature_url, &trusted)
            .map_err(|error| error.to_string())?;
        let installed_versions = UpdateEngine::new(paths.install_root)
            .and_then(|engine| engine.installed_versions())
            .map_err(|error| error.to_string())?;
        Ok(DesktopUpdateStatus {
            manifest: release.manifest,
            installed_versions,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn install_update(
    app: tauri::AppHandle,
    state: State<'_, Arc<DesktopServices>>,
    manifest_url: String,
    signature_url: String,
) -> Result<(), String> {
    let paths = state.paths.clone();
    if !paths.updater.is_file() {
        return Err(format!(
            "update helper is missing: {}",
            paths.updater.display()
        ));
    }
    let trusted: TrustedPublicKey = serde_json::from_slice(
        &std::fs::read(&paths.trusted_update_key).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let manifest_for_check = manifest_url.clone();
    let signature_for_check = signature_url.clone();
    tauri::async_runtime::spawn_blocking(move || {
        fetch_verified_release(&manifest_for_check, &signature_for_check, &trusted)
            .map(|_| ())
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())??;

    let mut command = std::process::Command::new(&paths.updater);
    command.args([
        "apply",
        "--root",
        &paths.install_root.to_string_lossy(),
        "--manifest-url",
        &manifest_url,
        "--signature-url",
        &signature_url,
        "--public-key",
        &paths.trusted_update_key.to_string_lossy(),
        "--wait-pid",
        &std::process::id().to_string(),
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.spawn().map_err(|error| error.to_string())?;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(350));
        app.exit(0);
    });
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let paths = AppPaths::discover(app.handle()).map_err(std::io::Error::other)?;
            // Set before BGE-M3 is initialized. The desktop app owns this process environment.
            unsafe { std::env::set_var("ORT_DYLIB_PATH", &paths.onnx_runtime) };
            app.manage(Arc::new(DesktopServices::new(paths)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_runtime_status,
            set_runtime_preference,
            search_documents,
            ask_question,
            translate_source,
            list_documents,
            read_document_text,
            check_updates,
            install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running ILIA desktop");
}
