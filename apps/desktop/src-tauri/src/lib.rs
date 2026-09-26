use std::{
    collections::HashSet,
    net::TcpListener,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use ilia_core::{
    AnswerLanguage, AnswerResponse, CitationMetadata, CitationStyle, DocumentSummary,
    ResearchErrorCode, ResearchEvent, ResearchMode, ResearchRequest, SearchOptions, SearchRequest,
    SearchResponse, format_legal_citation,
};
use ilia_database::{
    ApplicationDatabasePaths, CommitImport, Database, ImportPreview, ImportedDocument,
    NewSavedEvidence, Project, UserDatabase, UserLibrary, WorkspaceDatabase, WorkspaceExport,
};
use ilia_embedding::{BGE_M3_MODEL_ID, BgeM3Embedder};
use ilia_inference::{
    AnswerService, AutoManagedLlamaServer, CancellationToken, InferenceError, PerformancePreset,
    RuntimeManager, RuntimeManagerConfig, RuntimePreference, RuntimeProbeReport,
    RuntimeStartupReport, TranslationResponse,
};
use ilia_retrieval::{RelatedDocumentRegistry, RetrievalService, TopicRegistry};
use ilia_updater::{
    InstalledVersions, ProxyConfig, TrustedPublicKey, UpdateDiagnostic, UpdateEngine,
    UpdateManifest, diagnose_update_error, fetch_verified_release_with_proxy, redact_url,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

#[derive(Debug, Clone)]
struct AppPaths {
    install_root: PathBuf,
    app_data_dir: PathBuf,
    core_database: PathBuf,
    user_database: PathBuf,
    workspace_database: PathBuf,
    bge_cache: PathBuf,
    qwen_model: PathBuf,
    runtime_root: PathBuf,
    onnx_runtime: PathBuf,
    log_dir: PathBuf,
    updater: PathBuf,
    trusted_update_key: PathBuf,
    proxy_config: PathBuf,
    normalized_corpus: PathBuf,
    document_topics: PathBuf,
    document_relations: PathBuf,
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
        let app_data_dir = std::env::var_os("ILIA_APP_DATA_DIR")
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(|| {
                app.path().app_data_dir().map_err(|error| {
                    format!("cannot resolve ILIA application data directory: {error}")
                })
            })?;
        let preferred_core = root.join("data/core.sqlite");
        let core_database = if preferred_core.is_file() {
            preferred_core
        } else {
            root.join("data/ilia.sqlite3")
        };
        let databases = ApplicationDatabasePaths::initialize(&core_database, &app_data_dir)
            .map_err(|error| format!("cannot initialize ILIA databases: {error}"))?;
        let proxy_config = app_data_dir.join("update-proxy.json");
        let paths = Self {
            install_root: root.clone(),
            app_data_dir,
            core_database: databases.core,
            user_database: databases.user,
            workspace_database: databases.workspace,
            bge_cache: root.join("models/bge-m3"),
            qwen_model: root.join("models/qwen3-4b/Qwen3-4B-Q4_K_M.gguf"),
            runtime_root: root.join("runtime"),
            onnx_runtime: root.join("runtime/onnx/onnxruntime.dll"),
            log_dir,
            updater: development_root
                .map(|root| root.join("target/x86_64-pc-windows-gnu/release/ilia-updater.exe"))
                .unwrap_or_else(|| root.join("ilia-updater.exe")),
            trusted_update_key: root.join("update/trusted-key.json"),
            proxy_config,
            normalized_corpus: root.join("corpus/normalized"),
            document_topics: root.join("corpus/manifests/document_topics.v1.json"),
            document_relations: root.join("corpus/manifests/document_relations.v1.json"),
        };
        for (label, path) in [
            ("core database", &paths.core_database),
            ("user database", &paths.user_database),
            ("workspace database", &paths.workspace_database),
            ("BGE-M3 model", &paths.bge_cache),
            ("Qwen model", &paths.qwen_model),
            ("llama.cpp runtimes", &paths.runtime_root),
            ("ONNX Runtime", &paths.onnx_runtime),
            ("trusted update key", &paths.trusted_update_key),
            ("normalized legal-text library", &paths.normalized_corpus),
            ("document topic registry", &paths.document_topics),
            ("related-document registry", &paths.document_relations),
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
    performance_preset: Mutex<PerformancePreset>,
    active_research: Mutex<Option<ActiveResearch>>,
    last_model_use: Mutex<Instant>,
}

struct ActiveResearch {
    request_id: uuid::Uuid,
    cancellation: CancellationToken,
}

#[derive(Serialize)]
struct DesktopAskResponse {
    runtime: RuntimeStartupReport,
    search: SearchResponse,
    answer: AnswerResponse,
}

#[derive(Serialize)]
struct DesktopResearchResponse {
    request_id: uuid::Uuid,
    mode: ResearchMode,
    search: SearchResponse,
    runtime: Option<RuntimeStartupReport>,
    answer: Option<AnswerResponse>,
}

#[derive(Clone, Serialize)]
struct ResearchEventEnvelope {
    request_id: uuid::Uuid,
    event: ResearchEvent,
}

#[derive(Serialize)]
struct DesktopUpdateStatus {
    manifest: UpdateManifest,
    installed_versions: InstalledVersions,
    diagnostic: UpdateDiagnostic,
}

#[derive(Serialize)]
struct ProxySettingsView {
    enabled: bool,
    redacted_url: Option<String>,
}

#[derive(Serialize)]
struct DesktopTranslationResponse {
    runtime: RuntimeStartupReport,
    translation: TranslationResponse,
}

#[derive(Debug, Clone, Deserialize)]
struct SaveResearchRequest {
    project_id: String,
    conversation_title: String,
    question: String,
    answer: String,
    evidence: Vec<NewSavedEvidence>,
}

impl DesktopServices {
    fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            embedder: Mutex::new(None),
            runtime: Mutex::new(None),
            preference: Mutex::new(RuntimePreference::Auto),
            performance_preset: Mutex::new(PerformancePreset::Balanced),
            active_research: Mutex::new(None),
            last_model_use: Mutex::new(Instant::now()),
        }
    }

    fn search(&self, query: &str) -> Result<SearchResponse, String> {
        self.search_request(&SearchRequest {
            query: query.to_owned(),
            filters: Default::default(),
            limit: SearchOptions::default().limit,
            evidence_limit: SearchOptions::default().evidence_limit,
        })
    }

    fn search_request(&self, request: &SearchRequest) -> Result<SearchResponse, String> {
        let query = request.query.trim();
        if query.is_empty() {
            return Err("问题不能为空".to_owned());
        }
        let database = Database::open_read_only(&self.paths.core_database)
            .map_err(|error| error.to_string())?;
        let user_database = UserDatabase::open_read_only(&self.paths.user_database)
            .map_err(|error| error.to_string())?;
        let topics = TopicRegistry::from_path(&self.paths.document_topics)
            .map_err(|error| error.to_string())?;
        let retrieval = RetrievalService::new(database)
            .with_user_database(user_database)
            .with_topic_registry(topics);
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
        let options = SearchOptions {
            limit: request.limit.max(1),
            evidence_limit: request.evidence_limit,
            ..SearchOptions::default()
        };
        retrieval
            .search_hybrid_request(request, &vector, BGE_M3_MODEL_ID, options)
            .map_err(|error| error.to_string())
    }

    fn begin_research(&self, request_id: uuid::Uuid) -> Result<CancellationToken, String> {
        let mut active = self
            .active_research
            .lock()
            .map_err(|_| "active research lock is poisoned".to_owned())?;
        Ok(replace_active_research(&mut active, request_id))
    }

    fn finish_research(&self, request_id: uuid::Uuid) {
        if let Ok(mut active) = self.active_research.lock()
            && active
                .as_ref()
                .is_some_and(|current| current.request_id == request_id)
        {
            active.take();
        }
    }

    fn cancel_research(&self, request_id: Option<uuid::Uuid>) -> Result<bool, String> {
        let active = self
            .active_research
            .lock()
            .map_err(|_| "active research lock is poisoned".to_owned())?;
        Ok(cancel_active_research(&active, request_id))
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

    fn set_performance_preset(
        &self,
        preset: PerformancePreset,
    ) -> Result<RuntimeProbeReport, String> {
        *self
            .performance_preset
            .lock()
            .map_err(|_| "performance preset lock is poisoned".to_owned())? = preset;
        self.runtime
            .lock()
            .map_err(|_| "runtime state lock is poisoned".to_owned())?
            .take();
        self.runtime_status()
    }

    fn prewarm(&self) -> Result<RuntimeStartupReport, String> {
        let preference = *self
            .preference
            .lock()
            .map_err(|_| "runtime preference lock is poisoned".to_owned())?;
        let preset = *self
            .performance_preset
            .lock()
            .map_err(|_| "performance preset lock is poisoned".to_owned())?;
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| "runtime state lock is poisoned".to_owned())?;
        if runtime.is_none() {
            let api_key = uuid::Uuid::new_v4().simple().to_string();
            let server = RuntimeManager::start(&RuntimeManagerConfig {
                runtime_root: self.paths.runtime_root.clone(),
                model: self.paths.qwen_model.clone(),
                preference,
                performance_preset: preset,
                host: "127.0.0.1".to_owned(),
                port: available_loopback_port().map_err(|e| e.to_string())?,
                startup_timeout: Duration::from_secs(120),
                log_dir: self.paths.log_dir.clone(),
                api_key: Some(api_key.clone()),
            })
            .map_err(|e| e.to_string())?;
            *runtime = Some(RunningRuntime { server, api_key });
        }
        let report = runtime
            .as_ref()
            .expect("runtime initialized")
            .server
            .report()
            .clone();
        *self
            .last_model_use
            .lock()
            .map_err(|_| "model activity lock is poisoned".to_owned())? = Instant::now();
        Ok(report)
    }

    fn release_idle_model(&self, idle_seconds: u64) -> Result<bool, String> {
        if self
            .active_research
            .lock()
            .map_err(|_| "active research lock is poisoned".to_owned())?
            .is_some()
        {
            return Ok(false);
        }
        let idle = self
            .last_model_use
            .lock()
            .map_err(|_| "model activity lock is poisoned".to_owned())?
            .elapsed()
            >= Duration::from_secs(idle_seconds.max(60));
        if !idle {
            return Ok(false);
        }
        Ok(self
            .runtime
            .lock()
            .map_err(|_| "runtime state lock is poisoned".to_owned())?
            .take()
            .is_some())
    }

    fn ask(&self, query: &str) -> Result<DesktopAskResponse, String> {
        let search = self.search(query)?;
        let preference = *self
            .preference
            .lock()
            .map_err(|_| "runtime preference lock is poisoned".to_owned())?;
        let performance_preset = *self
            .performance_preset
            .lock()
            .map_err(|_| "performance preset lock is poisoned".to_owned())?;
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
                performance_preset,
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
        *self
            .last_model_use
            .lock()
            .map_err(|_| "model activity lock is poisoned".to_owned())? = Instant::now();
        Ok(DesktopAskResponse {
            runtime: runtime_report,
            search,
            answer,
        })
    }

    fn research<F>(
        &self,
        request: &ResearchRequest,
        cancellation: &CancellationToken,
        mut emit: F,
    ) -> Result<DesktopResearchResponse, String>
    where
        F: FnMut(ResearchEvent),
    {
        let question = request.question.trim();
        if question.is_empty() {
            return Err("问题不能为空".to_owned());
        }
        cancellation.check().map_err(|error| error.to_string())?;
        emit(ResearchEvent::Started);

        let (search, mut generation_question) = match request.mode {
            ResearchMode::Quick | ResearchMode::Standard => {
                let search = self.search_request(&SearchRequest {
                    query: question.to_owned(),
                    filters: request.filters.clone(),
                    limit: 10,
                    evidence_limit: if request.mode == ResearchMode::Quick {
                        10
                    } else {
                        5
                    },
                })?;
                (search, question.to_owned())
            }
            ResearchMode::Deep => {
                let subquestions = deep_research_subquestions(question);
                emit(ResearchEvent::PlanReady {
                    subquestions: subquestions.clone(),
                });
                let mut searches = Vec::with_capacity(subquestions.len());
                for subquestion in &subquestions {
                    cancellation.check().map_err(|error| error.to_string())?;
                    searches.push(self.search_request(&SearchRequest {
                        query: subquestion.clone(),
                        filters: request.filters.clone(),
                        limit: 10,
                        evidence_limit: 5,
                    })?);
                }
                (
                    merge_deep_search(question, searches, 10, 24_000),
                    deep_generation_question(question, &subquestions),
                )
            }
        };
        generation_question.push_str(match request.answer_language {
            AnswerLanguage::Chinese => "\n\n请仅使用简体中文回答。",
            AnswerLanguage::English => "\n\nAnswer only in English.",
            AnswerLanguage::FollowQuestion => "\n\n回答语言必须跟随用户问题的主要语言。",
        });
        cancellation.check().map_err(|error| error.to_string())?;
        emit(ResearchEvent::RetrievalCompleted {
            evidence_count: search.evidence.len(),
        });

        if request.mode == ResearchMode::Quick {
            emit(ResearchEvent::Completed);
            return Ok(DesktopResearchResponse {
                request_id: request.request_id,
                mode: request.mode,
                search,
                runtime: None,
                answer: None,
            });
        }

        let preference = *self
            .preference
            .lock()
            .map_err(|_| "runtime preference lock is poisoned".to_owned())?;
        let performance_preset = *self
            .performance_preset
            .lock()
            .map_err(|_| "performance preset lock is poisoned".to_owned())?;
        let mut runtime_guard = self
            .runtime
            .lock()
            .map_err(|_| "runtime state lock is poisoned".to_owned())?;
        cancellation.check().map_err(|error| error.to_string())?;
        if runtime_guard.is_none() {
            let api_key = uuid::Uuid::new_v4().simple().to_string();
            let server = RuntimeManager::start_cancellable(
                &RuntimeManagerConfig {
                    runtime_root: self.paths.runtime_root.clone(),
                    model: self.paths.qwen_model.clone(),
                    preference,
                    performance_preset,
                    host: "127.0.0.1".to_owned(),
                    port: available_loopback_port().map_err(|error| error.to_string())?,
                    startup_timeout: Duration::from_secs(120),
                    log_dir: self.paths.log_dir.clone(),
                    api_key: Some(api_key.clone()),
                },
                cancellation,
            )
            .map_err(|error| error.to_string())?;
            cancellation.check().map_err(|error| error.to_string())?;
            *runtime_guard = Some(RunningRuntime { server, api_key });
        }
        let running = runtime_guard.as_ref().expect("runtime was initialized");
        let runtime_report = running.server.report().clone();
        let answer_service =
            AnswerService::new(running.server.base_url()).with_api_key(&running.api_key);
        let draft = answer_service
            .answer_streaming(
                &generation_question,
                &search.evidence,
                cancellation,
                |text| {
                    emit(ResearchEvent::AnswerDelta {
                        text: text.to_owned(),
                    })
                },
            )
            .map_err(|error| error.to_string())?;
        let draft_text = draft.answer.clone();
        let mut answer = answer_service
            .audit_and_rewrite(question, &search.evidence, draft, cancellation)
            .map_err(|error| error.to_string())?;
        *self
            .last_model_use
            .lock()
            .map_err(|_| "model activity lock is poisoned".to_owned())? = Instant::now();
        answer.question = question.to_owned();
        if answer.answer != draft_text {
            emit(ResearchEvent::AnswerReplaced {
                text: answer.answer.clone(),
            });
        }
        emit(ResearchEvent::CitationAuditCompleted {
            findings: answer.citation_findings.clone(),
        });
        emit(ResearchEvent::Completed);
        Ok(DesktopResearchResponse {
            request_id: request.request_id,
            mode: request.mode,
            search,
            runtime: Some(runtime_report),
            answer: Some(answer),
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
        let performance_preset = *self
            .performance_preset
            .lock()
            .map_err(|_| "performance preset lock is poisoned".to_owned())?;
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
                performance_preset,
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
        *self
            .last_model_use
            .lock()
            .map_err(|_| "model activity lock is poisoned".to_owned())? = Instant::now();
        Ok(DesktopTranslationResponse {
            runtime: runtime_report,
            translation,
        })
    }

    fn documents(&self) -> Result<Vec<DocumentSummary>, String> {
        let mut documents = Database::open_read_only(&self.paths.core_database)
            .map_err(|error| error.to_string())?
            .documents()
            .map_err(|error| error.to_string())?;
        documents.extend(
            UserDatabase::open_read_only(&self.paths.user_database)
                .map_err(|e| e.to_string())?
                .documents()
                .map_err(|e| e.to_string())?,
        );
        Ok(documents)
    }

    fn related_documents(&self, document_id: &str) -> Result<Vec<DocumentSummary>, String> {
        let registry = RelatedDocumentRegistry::from_path(&self.paths.document_relations)
            .map_err(|e| e.to_string())?;
        let database =
            Database::open_read_only(&self.paths.core_database).map_err(|e| e.to_string())?;
        registry
            .related_to(document_id)
            .iter()
            .map(|id| {
                database
                    .document(id)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("related document is missing: {id}"))
            })
            .collect()
    }

    fn read_document_text(&self, document_id: &str) -> Result<String, String> {
        if document_id.is_empty() || document_id.contains(['/', '\\']) || document_id.contains("..")
        {
            return Err("invalid document identifier".to_owned());
        }
        let database = Database::open_read_only(&self.paths.core_database)
            .map_err(|error| error.to_string())?;
        if database
            .document(document_id)
            .map_err(|error| error.to_string())?
            .is_none()
        {
            return UserDatabase::open_read_only(&self.paths.user_database)
                .map_err(|error| error.to_string())?
                .document_text(document_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "document does not exist in the local library".to_owned());
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

fn replace_active_research(
    active: &mut Option<ActiveResearch>,
    request_id: uuid::Uuid,
) -> CancellationToken {
    if let Some(previous) = active.take() {
        previous.cancellation.cancel();
    }
    let cancellation = CancellationToken::default();
    *active = Some(ActiveResearch {
        request_id,
        cancellation: cancellation.clone(),
    });
    cancellation
}

fn cancel_active_research(active: &Option<ActiveResearch>, request_id: Option<uuid::Uuid>) -> bool {
    let Some(active) = active.as_ref() else {
        return false;
    };
    if request_id.is_none_or(|request_id| request_id == active.request_id) {
        active.cancellation.cancel();
        return true;
    }
    false
}

fn deep_research_subquestions(question: &str) -> Vec<String> {
    vec![
        format!("该问题适用的主要国际法规则、文书和条款是什么：{question}"),
        format!("国际法院或其他相关裁判如何解释和适用这些规则：{question}"),
        format!("这些规则的构成要件、例外、限制和法律后果是什么：{question}"),
    ]
}

fn deep_generation_question(question: &str, subquestions: &[String]) -> String {
    format!(
        "请就下列国际法研究问题形成有证据支持的研究答复：\n{question}\n\n研究子问题：\n{}\n\n请严格使用六段结构：一、问题与范围；二、适用法律；三、主要裁判与解释；四、分析；五、限制与不确定性；六、结论。每个实质句均须引用证据。",
        subquestions
            .iter()
            .enumerate()
            .map(|(index, item)| format!("{}. {item}", index + 1))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn merge_deep_search(
    question: &str,
    searches: Vec<SearchResponse>,
    evidence_limit: usize,
    character_budget: usize,
) -> SearchResponse {
    let mut document_keys = HashSet::new();
    let mut hit_keys = HashSet::new();
    let mut evidence_keys = HashSet::new();
    let mut documents = Vec::new();
    let mut hits = Vec::new();
    let mut evidence = Vec::new();
    let mut used_characters = 0usize;

    for search in searches {
        for document in search.documents {
            if document_keys.insert(document.stable_document_key.clone()) {
                documents.push(document);
            }
        }
        for hit in search.hits {
            if hit_keys.insert(hit.stable_key.to_string()) {
                hits.push(hit);
            }
        }
        for mut item in search.evidence {
            let key = format!("{}\u{1f}{}", item.citation_label, item.text);
            if evidence.len() >= evidence_limit
                || !evidence_keys.insert(key)
                || used_characters + item.text.chars().count() > character_budget
            {
                continue;
            }
            used_characters += item.text.chars().count();
            item.rank = evidence.len() + 1;
            evidence.push(item);
        }
    }
    hits.sort_by(|left, right| right.score.total_cmp(&left.score));
    hits.truncate(20);
    SearchResponse {
        query: question.to_owned(),
        normalized_query: question.to_lowercase(),
        detected_document_id: None,
        detected_locator: None,
        documents,
        hits,
        evidence,
    }
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
async fn set_performance_preset(
    state: State<'_, Arc<DesktopServices>>,
    preset: String,
) -> Result<RuntimeProbeReport, String> {
    let preset = preset.parse::<PerformancePreset>()?;
    let services = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || services.set_performance_preset(preset))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn prewarm_model(
    state: State<'_, Arc<DesktopServices>>,
) -> Result<RuntimeStartupReport, String> {
    let services = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || services.prewarm())
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn release_idle_model(
    state: State<'_, Arc<DesktopServices>>,
    idle_seconds: u64,
) -> Result<bool, String> {
    state.release_idle_model(idle_seconds)
}

#[tauri::command]
async fn list_projects(state: State<'_, Arc<DesktopServices>>) -> Result<Vec<Project>, String> {
    let path = state.paths.workspace_database.clone();
    tauri::async_runtime::spawn_blocking(move || WorkspaceDatabase::open(path)?.projects())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn create_project(
    state: State<'_, Arc<DesktopServices>>,
    title: String,
    description: String,
    tags: Vec<String>,
) -> Result<Project, String> {
    let path = state.paths.workspace_database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        WorkspaceDatabase::open(path)?.create_project(&title, &description, &tags)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn save_research(
    state: State<'_, Arc<DesktopServices>>,
    request: SaveResearchRequest,
) -> Result<WorkspaceExport, String> {
    let path = state.paths.workspace_database.clone();
    tauri::async_runtime::spawn_blocking(
        move || -> Result<WorkspaceExport, ilia_database::DatabaseError> {
            let mut database = WorkspaceDatabase::open(path)?;
            let conversation = database
                .create_conversation(Some(&request.project_id), &request.conversation_title)?;
            database.append_message(&conversation.id, "user", &request.question, &[])?;
            database.append_message(&conversation.id, "assistant", &request.answer, &[])?;
            for evidence in request.evidence {
                database.save_evidence(Some(&request.project_id), &evidence)?;
            }
            database.snapshot(&request.project_id)
        },
    )
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn save_note(
    state: State<'_, Arc<DesktopServices>>,
    project_id: String,
    note_id: Option<String>,
    body: String,
) -> Result<WorkspaceExport, String> {
    let path = state.paths.workspace_database.clone();
    tauri::async_runtime::spawn_blocking(
        move || -> Result<WorkspaceExport, ilia_database::DatabaseError> {
            let database = WorkspaceDatabase::open(path)?;
            database.upsert_note(note_id.as_deref(), Some(&project_id), None, None, &body)?;
            database.snapshot(&project_id)
        },
    )
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn export_project(
    state: State<'_, Arc<DesktopServices>>,
    project_id: String,
    format: String,
) -> Result<String, String> {
    let path = state.paths.workspace_database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let database = WorkspaceDatabase::open(path)?;
        match format.as_str() {
            "markdown" => database.export_markdown(&project_id),
            "html" => database.export_html(&project_id),
            _ => Err(ilia_database::DatabaseError::InvalidSchema),
        }
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_project(
    state: State<'_, Arc<DesktopServices>>,
    project_id: String,
) -> Result<WorkspaceExport, String> {
    let path = state.paths.workspace_database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        WorkspaceDatabase::open(path)?.snapshot(&project_id)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn prepare_import(
    state: State<'_, Arc<DesktopServices>>,
    path: String,
) -> Result<ImportPreview, String> {
    let database = state.paths.user_database.clone();
    let app_data = state.paths.app_data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        UserLibrary::open(database, app_data)?.prepare_import(path)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn commit_import(
    state: State<'_, Arc<DesktopServices>>,
    preview_id: String,
    title: String,
    language: String,
    document_type: String,
) -> Result<ImportedDocument, String> {
    let services = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let library =
            UserLibrary::open(&services.paths.user_database, &services.paths.app_data_dir)
                .map_err(|e| e.to_string())?;
        let chunks = library
            .preview_chunks(&preview_id)
            .map_err(|e| e.to_string())?;
        let texts = chunks
            .iter()
            .map(|chunk| chunk.text.clone())
            .collect::<Vec<_>>();
        let mut embedder = services
            .embedder
            .lock()
            .map_err(|_| "BGE-M3 state lock is poisoned".to_owned())?;
        if embedder.is_none() {
            *embedder = Some(
                BgeM3Embedder::new(services.paths.bge_cache.clone(), false)
                    .map_err(|e| e.to_string())?,
            );
        }
        let embeddings = embedder
            .as_mut()
            .expect("embedder initialized")
            .embed(&texts, 8)
            .map_err(|e| e.to_string())?;
        drop(embedder);
        library
            .commit_import(CommitImport {
                preview_id,
                title,
                language,
                document_type,
                model_id: BGE_M3_MODEL_ID.to_owned(),
                embeddings,
            })
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn cancel_import(
    state: State<'_, Arc<DesktopServices>>,
    preview_id: String,
) -> Result<(), String> {
    let database = state.paths.user_database.clone();
    let app_data = state.paths.app_data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        UserLibrary::open(database, app_data)?.cancel_preview(&preview_id)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_user_documents(
    state: State<'_, Arc<DesktopServices>>,
) -> Result<Vec<ImportedDocument>, String> {
    let database = state.paths.user_database.clone();
    let app_data = state.paths.app_data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || UserLibrary::open(database, app_data)?.documents())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_user_document(
    state: State<'_, Arc<DesktopServices>>,
    document_id: String,
) -> Result<bool, String> {
    let database = state.paths.user_database.clone();
    let app_data = state.paths.app_data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        UserLibrary::open(database, app_data)?.delete_document(&document_id)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn rebuild_user_index(state: State<'_, Arc<DesktopServices>>) -> Result<(), String> {
    let database = state.paths.user_database.clone();
    let app_data = state.paths.app_data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        UserLibrary::open(database, app_data)?.rebuild_fts()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
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
async fn start_research(
    app: tauri::AppHandle,
    state: State<'_, Arc<DesktopServices>>,
    request: ResearchRequest,
) -> Result<DesktopResearchResponse, String> {
    let services = state.inner().clone();
    let cancellation = services.begin_research(request.request_id)?;
    let request_id = request.request_id;
    let event_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        services.research(&request, &cancellation, |event| {
            let _ = event_app.emit(
                "research-event",
                ResearchEventEnvelope { request_id, event },
            );
        })
    })
    .await
    .map_err(|error| error.to_string())?;

    let services = state.inner().clone();
    services.finish_research(request_id);
    if let Err(error) = &result {
        let event = if error == &InferenceError::Cancelled.to_string() {
            ResearchEvent::Cancelled
        } else {
            ResearchEvent::Failed {
                code: ResearchErrorCode::GenerationFailed,
                safe_message: "研究请求未能完成，请检查本地模型与资料库状态。".to_owned(),
            }
        };
        let _ = app.emit(
            "research-event",
            ResearchEventEnvelope { request_id, event },
        );
    }
    result
}

#[tauri::command]
async fn cancel_research(
    state: State<'_, Arc<DesktopServices>>,
    request_id: Option<uuid::Uuid>,
) -> Result<bool, String> {
    state.cancel_research(request_id)
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
async fn related_documents(
    state: State<'_, Arc<DesktopServices>>,
    document_id: String,
) -> Result<Vec<DocumentSummary>, String> {
    let services = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || services.related_documents(&document_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn format_citation(
    title: String,
    locator: String,
    url: Option<String>,
    style: String,
) -> Result<String, String> {
    let style = match style.as_str() {
        "chinese" => CitationStyle::Chinese,
        "oscola" => CitationStyle::Oscola,
        "bluebook" => CitationStyle::Bluebook,
        "icj" => CitationStyle::Icj,
        "markdown" => CitationStyle::Markdown,
        "plain_text" => CitationStyle::PlainText,
        _ => return Err("unsupported citation style".into()),
    };
    Ok(format_legal_citation(
        &CitationMetadata {
            title,
            locator,
            url: url.filter(|value| !value.is_empty()),
            year: None,
            court: None,
            report: None,
        },
        style,
    ))
}

fn load_proxy(path: &std::path::Path) -> Result<Option<ProxyConfig>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    serde_json::from_slice(&std::fs::read(path).map_err(|error| error.to_string())?)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_proxy_settings(
    state: State<'_, Arc<DesktopServices>>,
) -> Result<ProxySettingsView, String> {
    let proxy = load_proxy(&state.paths.proxy_config)?;
    Ok(ProxySettingsView {
        enabled: proxy.is_some(),
        redacted_url: proxy.map(|value| redact_url(&value.url)),
    })
}

#[tauri::command]
async fn set_proxy_settings(
    state: State<'_, Arc<DesktopServices>>,
    url: Option<String>,
) -> Result<ProxySettingsView, String> {
    let path = &state.paths.proxy_config;
    if let Some(url) = url.filter(|value| !value.trim().is_empty()) {
        let parsed = url::Url::parse(url.trim()).map_err(|_| "代理 URL 无效".to_owned())?;
        if !matches!(parsed.scheme(), "http" | "https" | "socks5") {
            return Err("仅支持 HTTP、HTTPS 和 SOCKS5 代理".into());
        }
        if parsed.host_str().is_none() {
            return Err("代理 URL 缺少主机".into());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let temporary = path.with_extension("json.new");
        std::fs::write(
            &temporary,
            serde_json::to_vec_pretty(&ProxyConfig {
                url: url.trim().to_owned(),
            })
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        std::fs::rename(temporary, path).map_err(|e| e.to_string())?;
    } else if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    get_proxy_settings(state).await
}

#[tauri::command]
async fn check_updates(
    state: State<'_, Arc<DesktopServices>>,
    manifest_url: String,
    signature_url: String,
) -> Result<DesktopUpdateStatus, UpdateDiagnostic> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let trusted: TrustedPublicKey = serde_json::from_slice(
            &std::fs::read(&paths.trusted_update_key).map_err(ilia_updater::UpdateError::from)?,
        )
        .map_err(ilia_updater::UpdateError::from)?;
        let proxy =
            load_proxy(&paths.proxy_config).map_err(ilia_updater::UpdateError::InvalidManifest)?;
        let release = fetch_verified_release_with_proxy(
            &manifest_url,
            &signature_url,
            &trusted,
            proxy.as_ref(),
        )?;
        let installed_versions =
            UpdateEngine::new(paths.install_root).and_then(|engine| engine.installed_versions())?;
        let current = release.manifest.components.iter().all(|component| {
            installed_versions.components.get(&component.id) == Some(&component.version)
        });
        Ok::<_, ilia_updater::UpdateError>(DesktopUpdateStatus {
            manifest: release.manifest,
            installed_versions,
            diagnostic: if current {
                UpdateDiagnostic {
                    code: "up_to_date".into(),
                    message_zh: "当前已经是最新版本。".into(),
                }
            } else {
                UpdateDiagnostic {
                    code: "update_available".into(),
                    message_zh: "发现可安装更新。".into(),
                }
            },
        })
    })
    .await
    .map_err(|_| UpdateDiagnostic {
        code: "server_unreachable".into(),
        message_zh: "更新检查任务异常终止。".into(),
    })?
    .map_err(|error| diagnose_update_error(&error))
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
    let proxy_config_for_check = paths.proxy_config.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let proxy = load_proxy(&proxy_config_for_check)?;
        fetch_verified_release_with_proxy(
            &manifest_for_check,
            &signature_for_check,
            &trusted,
            proxy.as_ref(),
        )
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
    if paths.proxy_config.is_file() {
        command.args(["--proxy-config", &paths.proxy_config.to_string_lossy()]);
    }
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

#[tauri::command]
async fn install_local_update(
    app: tauri::AppHandle,
    state: State<'_, Arc<DesktopServices>>,
    package_path: String,
) -> Result<(), String> {
    let paths = state.paths.clone();
    let package = PathBuf::from(package_path);
    if !package.is_file() {
        return Err("本地更新包不存在".into());
    }
    if package.extension().and_then(|v| v.to_str()) != Some("ilia") {
        return Err("请选择 .ilia 更新包".into());
    }
    let mut command = std::process::Command::new(&paths.updater);
    command.args([
        "apply-package",
        "--root",
        &paths.install_root.to_string_lossy(),
        "--package",
        &package.to_string_lossy(),
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
    command.spawn().map_err(|e| e.to_string())?;
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
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let services = window.state::<Arc<DesktopServices>>();
                let _ = services.cancel_research(None);
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_runtime_status,
            set_runtime_preference,
            set_performance_preset,
            prewarm_model,
            release_idle_model,
            search_documents,
            ask_question,
            start_research,
            cancel_research,
            translate_source,
            list_documents,
            read_document_text,
            related_documents,
            format_citation,
            list_projects,
            create_project,
            save_research,
            save_note,
            export_project,
            get_project,
            prepare_import,
            commit_import,
            cancel_import,
            list_user_documents,
            delete_user_document,
            rebuild_user_index,
            get_proxy_settings,
            set_proxy_settings,
            check_updates,
            install_update,
            install_local_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running ILIA desktop");
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilia_core::EvidenceItem;

    fn search_with_evidence(items: Vec<EvidenceItem>) -> SearchResponse {
        SearchResponse {
            query: "subquestion".to_owned(),
            normalized_query: "subquestion".to_owned(),
            detected_document_id: None,
            detected_locator: None,
            documents: Vec::new(),
            hits: Vec::new(),
            evidence: items,
        }
    }

    fn evidence(rank: usize, text: &str) -> EvidenceItem {
        EvidenceItem {
            rank,
            stable_key: ilia_core::StableEvidenceKey::new(
                ilia_core::LibraryKind::Core,
                "document",
                format!("chunk-{rank}"),
            )
            .unwrap(),
            chunk_id: format!("chunk-{rank}"),
            related_chunk_ids: Vec::new(),
            citation_label: format!("Citation {rank}"),
            selection_reason: "test".to_owned(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn deep_mode_has_a_bounded_non_recursive_plan() {
        let plan = deep_research_subquestions("国家责任如何成立？");
        assert!((3..=5).contains(&plan.len()));
        assert!(plan.iter().all(|item| item.contains("国家责任如何成立？")));
    }

    #[test]
    fn deep_merge_deduplicates_and_respects_evidence_budget() {
        let duplicate = evidence(1, "same evidence");
        let merged = merge_deep_search(
            "question",
            vec![
                search_with_evidence(vec![duplicate.clone()]),
                search_with_evidence(vec![duplicate, evidence(2, "second")]),
            ],
            2,
            100,
        );
        assert_eq!(merged.evidence.len(), 2);
        assert_eq!(merged.evidence[0].rank, 1);
        assert_eq!(merged.evidence[1].rank, 2);
    }

    #[test]
    fn replacing_a_request_cancels_the_previous_one() {
        let mut active = None;
        let first_id = uuid::Uuid::new_v4();
        let first = replace_active_research(&mut active, first_id);
        let second_id = uuid::Uuid::new_v4();
        let second = replace_active_research(&mut active, second_id);
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());
        assert!(!cancel_active_research(&active, Some(first_id)));
        assert!(cancel_active_research(&active, None));
        assert!(second.is_cancelled());
    }
}
