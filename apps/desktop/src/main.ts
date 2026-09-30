import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./styles.css";

type Backend = "auto" | "cuda" | "vulkan" | "cpu";
type ResearchMode = "quick" | "standard" | "deep";
type AnswerLanguage = "chinese" | "english" | "follow_question";
type PerformancePreset = "energy_saver" | "balanced" | "high_performance";

interface RuntimeDevice {
  id: string;
  name: string;
  total_memory_mib: number | null;
  free_memory_mib: number | null;
}

interface BackendProbe {
  backend: Exclude<Backend, "auto">;
  available: boolean;
  devices: RuntimeDevice[];
  recommended_device: string | null;
  diagnostic: string;
}

interface RuntimeProbeReport {
  preference: Backend;
  fallback_order: Exclude<Backend, "auto">[];
  recommended_backend: Exclude<Backend, "auto"> | null;
  probes: BackendProbe[];
}

interface RuntimeStartupReport {
  detection: RuntimeProbeReport;
  selected_backend: Exclude<Backend, "auto">;
  selected_profile: {
    context_size: number;
    gpu_layers: number;
    estimated_memory_mib: number;
    device: string | null;
  };
  attempts: Array<{
    backend: Exclude<Backend, "auto">;
    started: boolean;
    error: string | null;
  }>;
}

interface SearchHit {
  chunk_id: string;
  document_id: string;
  library_kind: "core" | "user";
  stable_key: string;
  canonical_title: string;
  title_zh: string | null;
  document_type: string;
  legal_status: string;
  official_source_url: string;
  citation_label: string;
  page_start: number;
  page_end: number;
  language: string;
  text: string;
  match_kind: string;
  score: number;
}

interface EvidenceItem {
  rank: number;
  stable_key: string;
  chunk_id: string;
  related_chunk_ids: string[];
  citation_label: string;
  selection_reason: string;
  text: string;
}

interface SearchResponse {
  query: string;
  hits: SearchHit[];
  evidence: EvidenceItem[];
}

interface CitationFinding {
  statement: string;
  evidence_numbers: number[];
  support: "direct" | "summary" | "unsupported" | "conflict";
}

interface AnswerResponse {
  answer: string;
  grounded: boolean;
  citations: Array<{
    evidence_number: number;
    stable_key: string;
    chunk_id: string;
    citation_label: string;
  }>;
  evidence: EvidenceItem[];
  generation_ms: number;
  warnings: string[];
  citation_findings: CitationFinding[];
  citation_rewritten: boolean;
}

interface ResearchResponse {
  request_id: string;
  mode: ResearchMode;
  search: SearchResponse;
  runtime: RuntimeStartupReport | null;
  answer: AnswerResponse | null;
}

type ResearchEvent =
  | { type: "started" }
  | { type: "retrieval_completed"; data: { evidence_count: number } }
  | { type: "plan_ready"; data: { subquestions: string[] } }
  | { type: "answer_delta"; data: { text: string } }
  | { type: "answer_replaced"; data: { text: string } }
  | { type: "citation_audit_completed"; data: { findings: CitationFinding[] } }
  | { type: "completed" }
  | { type: "cancelled" }
  | { type: "failed"; data: { code: string; safe_message: string } };

interface ResearchEventEnvelope {
  request_id: string;
  event: ResearchEvent;
}

interface TranslationResponse {
  runtime: RuntimeStartupReport;
  translation: {
    translated_text: string;
    model_id: string;
    generation_ms: number;
  };
}

interface DocumentSummary {
  document_id: string;
  library_kind: "core" | "user";
  stable_document_key: string;
  canonical_title: string;
  title_zh: string | null;
  short_title: string | null;
  document_type: string;
  legal_status: string;
  official_source_url: string;
}

interface UpdateComponent {
  id: string;
  kind: "application" | "corpus" | "model" | "runtime";
  version: string;
  from_version: string | null;
  payload_size: number;
}

interface UpdateStatus {
  manifest: { release_id: string; components: UpdateComponent[] };
  installed_versions: { components: Record<string, string> };
}

interface Project { id: string; title: string; description: string; tags: string[] }
interface ImportPreview { preview_id: string; source_filename: string; source_sha256: string; byte_length: number; inferred_title: string; inferred_language: string; inferred_document_type: string; text_preview: string; chunk_count: number }
interface ImportedDocument { id: string; title: string; source_filename: string; source_sha256: string; byte_length: number; chunk_count: number }
interface ProxySettings { enabled: boolean; redacted_url: string | null; source: "manual" | "environment" | "system" | null }
interface BackupSummary { path: string; user_bytes: number; workspace_bytes: number; user_sha256: string; workspace_sha256: string }
interface UpdateSummary {
  current_version: string;
  last_release_id: string | null;
  last_status: "staged" | "applying" | "applied" | "rolling_back" | "rolled_back" | "failed" | null;
  applied_components: string[];
}
interface UpdateProgress {
  phase: "checking" | "downloading" | "verifying" | "ready_to_apply" | "applying" | "applied" | "cancelled" | "failed";
  release_id: string;
  component_id: string | null;
  downloaded_bytes: number;
  total_bytes: number;
  message_zh: string;
}
const updateManifestUrl = "https://github.com/LeoLiao0806Xuan/ILIA/releases/latest/download/update-manifest.json";
const updateSignatureUrl = "https://github.com/LeoLiao0806Xuan/ILIA/releases/latest/download/update-manifest.sig";

const demoHit: SearchHit = {
  chunk_id: "unclos-1982-art-3-en",
  document_id: "unclos-1982",
  library_kind: "core",
  stable_key: "core:unclos-1982:unclos-1982-art-3-en",
  canonical_title: "United Nations Convention on the Law of the Sea",
  title_zh: "联合国海洋法公约",
  document_type: "treaty",
  legal_status: "in_force_treaty",
  official_source_url: "https://www.un.org/depts/los/convention_agreements/texts/unclos/unclos_e.pdf",
  citation_label: "UNCLOS, Article 3",
  page_start: 21,
  page_end: 21,
  language: "en",
  text: "Article 3\nBreadth of the territorial sea\nEvery State has the right to establish the breadth of its territorial sea up to a limit not exceeding 12 nautical miles, measured from baselines determined in accordance with this Convention.",
  match_kind: "hybrid",
  score: 0.0325,
};

const demoRuntime: RuntimeProbeReport = {
  preference: "auto",
  fallback_order: ["cuda", "vulkan", "cpu"],
  recommended_backend: "cuda",
  probes: [
    {
      backend: "cuda",
      available: true,
      devices: [{ id: "CUDA0", name: "NVIDIA GeForce RTX 4070 Laptop GPU", total_memory_mib: 8187, free_memory_mib: 7068 }],
      recommended_device: "CUDA0",
      diagnostic: "Available devices: CUDA0",
    },
    { backend: "vulkan", available: true, devices: [], recommended_device: "Vulkan1", diagnostic: "Available" },
    { backend: "cpu", available: true, devices: [], recommended_device: "none", diagnostic: "Available" },
  ],
};

const demoDocuments: DocumentSummary[] = [{
  document_id: "unclos-1982",
  library_kind: "core",
  stable_document_key: "core:unclos-1982",
  canonical_title: "United Nations Convention on the Law of the Sea",
  title_zh: "联合国海洋法公约",
  short_title: "UNCLOS",
  document_type: "treaty",
  legal_status: "in_force_treaty",
  official_source_url: demoHit.official_source_url,
}];

const isTauri = () => "__TAURI_INTERNALS__" in window;

async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (isTauri()) return invoke<T>(command, args);
  await new Promise((resolve) => window.setTimeout(resolve, command === "ask_question" ? 650 : 220));
  if (command === "get_runtime_status" || command === "set_runtime_preference" || command === "set_performance_preset") return demoRuntime as T;
  if (command === "prewarm_model") return { detection: demoRuntime, selected_backend: "cuda", selected_profile: { context_size: 16384, gpu_layers: 99, estimated_memory_mib: 5200, device: "CUDA0" }, attempts: [] } as T;
  if (command === "list_projects") return [] as T;
  if (command === "list_user_documents") return [] as T;
  if (command === "get_proxy_settings") return { enabled: false, redacted_url: null, source: null } as T;
  if (command === "create_backup" || command === "restore_backup") return { path: String(args.outputPath ?? args.packagePath ?? "ILIA-backup.ilia-workspace"), user_bytes: 32768, workspace_bytes: 65536, user_sha256: "demo", workspace_sha256: "demo" } as T;
  if (command === "get_update_summary") return { current_version: "1.1.10", last_release_id: "v1.1.9", last_status: "applied", applied_components: ["application", "updater"] } as T;
  if (command === "check_updates") return { manifest: { release_id: "demo", components: [] }, installed_versions: { components: {} } } as T;
  if (command === "inspect_local_update") return { manifest: { release_id: "v1.1.10", components: [{ id: "application", kind: "application", version: "1.1.10", from_version: null, payload_size: 41840986 }] }, installed_versions: { components: { application: "1.1.9" } } } as T;
  if (command === "install_update" || command === "install_local_update") return undefined as T;
  if (command === "cancel_update") return true as T;
  if (command === "cancel_research") return true as T;
  if (command === "list_documents") return demoDocuments as T;
  if (command === "read_document_text") return `ILIA NORMALIZED LEGAL TEXT\n\n${demoHit.text}` as T;
  if (command === "related_documents") return [] as T;
  if (command === "format_citation") return `${String(args.title)} — ${String(args.locator)}` as T;
  const search: SearchResponse = { query: String(args.query ?? ""), hits: [demoHit], evidence: [{ rank: 1, stable_key: demoHit.stable_key, chunk_id: demoHit.chunk_id, related_chunk_ids: [], citation_label: demoHit.citation_label, selection_reason: "RRF fusion of FTS5 and BGE-M3", text: demoHit.text }] };
  if (command === "search_documents") return search as T;
  if (command === "start_research") {
    const request = args.request as { request_id: string; mode: ResearchMode; question: string };
    return {
      request_id: request.request_id,
      mode: request.mode,
      search,
      runtime: request.mode === "quick" ? null : { detection: demoRuntime, selected_backend: "cuda", selected_profile: { context_size: 16384, gpu_layers: 99, estimated_memory_mib: 5200, device: "CUDA0" }, attempts: [{ backend: "cuda", started: true, error: null }] },
      answer: request.mode === "quick" ? null : { answer: "《联合国海洋法公约》规定，领海宽度不得超过 12 海里【1】。", grounded: true, evidence: search.evidence, generation_ms: 579, warnings: [], citations: [{ evidence_number: 1, stable_key: demoHit.stable_key, chunk_id: demoHit.chunk_id, citation_label: demoHit.citation_label }], citation_findings: [{ statement: "领海宽度不得超过 12 海里", evidence_numbers: [1], support: "direct" }], citation_rewritten: false },
    } as T;
  }
  if (command === "translate_source") return {
    runtime: { detection: demoRuntime, selected_backend: "cuda", selected_profile: { context_size: 16384, gpu_layers: 99, estimated_memory_mib: 5200, device: "CUDA0" }, attempts: [{ backend: "cuda", started: true, error: null }] },
    translation: { translated_text: "第三条\n领海的宽度\n每一国家有权确定其领海宽度，直至从按照本公约确定的基线量起不超过十二海里的界限。", model_id: "Qwen3-4B", generation_ms: 438 },
  } as T;
  return {
    runtime: { detection: demoRuntime, selected_backend: "cuda", selected_profile: { context_size: 16384, gpu_layers: 99, estimated_memory_mib: 5200, device: "CUDA0" }, attempts: [{ backend: "cuda", started: true, error: null }] },
    search,
    answer: { answer: "《联合国海洋法公约》规定，领海宽度不得超过 12 海里【1】。", grounded: true, evidence: search.evidence, generation_ms: 579, warnings: [], citations: [{ evidence_number: 1, stable_key: demoHit.stable_key, chunk_id: demoHit.chunk_id, citation_label: demoHit.citation_label }], citation_findings: [{ statement: "领海宽度不得超过 12 海里", evidence_numbers: [1], support: "direct" }], citation_rewritten: false },
  } as T;
}

document.querySelector<HTMLDivElement>("#app")!.innerHTML = `
  <div class="shell">
    <aside class="app-rail" aria-label="主导航">
      <div class="rail-brand" aria-label="ILIA 国际法研究助手">
        <strong>ILIA</strong>
        <span>国际法研究</span>
      </div>
      <nav>
        <button class="rail-button active" type="button" aria-current="page"><span class="rail-icon" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M4 19.5V6.8c0-.7.5-1.3 1.2-1.4L12 4v15l-6.8 1.4A1 1 0 0 1 4 19.5Zm16 0V6.8c0-.7-.5-1.3-1.2-1.4L12 4v15l6.8 1.4a1 1 0 0 0 1.2-.9Z"/></svg></span><span>研究</span></button>
        <button class="rail-button" id="projects-button" type="button"><span class="rail-icon" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M4 7.5h6l1.6 2H20v9.5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7.5Zm0 0V5a1 1 0 0 1 1-1h4l1.5 2H19a1 1 0 0 1 1 1v2.5"/></svg></span><span>项目</span></button>
        <button class="rail-button" id="library-button" type="button"><span class="rail-icon" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M5 4h4v16H5V4Zm5.5 0h4v16h-4V4Zm5.5 1 3.5-1 3.5 14.5-3.5 1L16 5Z"/></svg></span><span>资料库</span></button>
        <button class="rail-button" id="update-button" type="button"><span class="rail-icon" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M20 7v5h-5M4 17v-5h5m10.2-2A8 8 0 0 0 6.5 6.5L4 9m16 6-2.5 2.5A8 8 0 0 1 4.8 14"/></svg></span><span id="update-button-label">检查更新</span></button>
      </nav>
      <button class="rail-button rail-guide" id="guide-button" type="button"><span class="rail-icon" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M9.4 18h5.2M10 21h4M8.2 14.8A6 6 0 1 1 15.8 14c-.9.7-1.3 1.4-1.3 2H9.6c0-.6-.4-1.2-1.4-1.9Z"/></svg></span><span>使用指南</span></button>
      <button class="rail-button rail-settings" id="settings-button" type="button"><span class="rail-icon" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M12 15.2a3.2 3.2 0 1 0 0-6.4 3.2 3.2 0 0 0 0 6.4Zm7-3.2 1.5-1.2-1.8-3.1-1.8.7a7.2 7.2 0 0 0-1.8-1L14.8 5h-3.6l-.3 2.4c-.6.2-1.2.6-1.8 1l-1.8-.7-1.8 3.1L7 12c0 .7.1 1.4.3 2l-1.5 1.2 1.8 3.1 1.8-.7c.5.4 1.1.8 1.8 1l.3 2.4h3.6l.3-2.4c.6-.2 1.2-.6 1.8-1l1.8.7 1.8-3.1-1.5-1.2c.1-.6.2-1.3.2-2Z"/></svg></span><span>设置</span></button>
    </aside>

    <aside class="project-sidebar">
      <div class="sidebar-brand"><span class="section-kicker">研究空间</span><strong>ILIA</strong><small>International Law Intelligence Assistant</small></div>
      <section class="sidebar-section">
        <div class="sidebar-label"><span>当前项目</span><button id="project-manage-shortcut" type="button">管理</button></div>
        <select id="project-select" aria-label="当前项目"><option value="">未选择项目</option></select>
      </section>
      <section class="sidebar-section sidebar-grow">
        <div class="sidebar-label"><span>研究起点</span></div>
        <button class="example history-item active" data-question="《联合国海洋法公约》如何规定领海宽度与基线？"><strong>领海宽度与基线</strong><small>UNCLOS · 条约解释</small></button>
        <button class="example history-item" data-question="一国能否援引其国内法作为不履行条约的理由？"><strong>国家能否援引国内法</strong><small>VCLT · 条约义务</small></button>
        <button class="example history-item" data-question="尼加拉瓜案第191段如何说明国际习惯法中的法律确信？"><strong>尼加拉瓜案第191段</strong><small>ICJ · 法律确信</small></button>
        <button class="example history-item" data-question="《联合国宪章》第51条对自卫权规定了哪些条件？"><strong>《联合国宪章》第51条</strong><small>自卫权 · 武力使用</small></button>
      </section>
      <div class="runtime-card" id="runtime-pill">
        <div><span class="pulse"></span><strong id="runtime-text">正在探测运行环境</strong></div>
        <label class="backend-control">运行方式
          <select id="backend-select" aria-label="运行方式">
            <option value="auto">自动选择</option><option value="cuda">NVIDIA CUDA</option><option value="vulkan">通用 Vulkan</option><option value="cpu">仅 CPU</option>
          </select>
        </label>
      </div>
    </aside>

    <main class="research-main">
      <header class="workspace-bar">
        <div class="breadcrumb"><span>研究工作台</span><span aria-hidden="true">›</span><strong id="workspace-context">新研究</strong></div>
        <div class="workspace-actions"><span class="offline-state">● 完全离线</span><button class="secondary compact" id="export-current-shortcut" type="button">导出项目</button></div>
      </header>
      <section class="research-canvas">
        <div class="research-heading">
          <div><div class="section-kicker">新研究</div><h1>从问题到可核验结论</h1><p>在同一工作流中完成检索、回答、引证复核与原文核验。</p></div>
        </div>
        <div id="first-run-tip" class="first-run-tip hidden"><div><strong>第一次使用？</strong><span>用 2 分钟了解从提问、核验证据到保存案件的完整流程。</span></div><button id="first-run-guide" class="secondary" type="button">打开指南</button><button id="first-run-dismiss" type="button" aria-label="关闭新手提示">×</button></div>
        <div class="query-composer">
          <label class="question-label" for="question">研究问题</label>
          <textarea id="question" rows="3">《联合国海洋法公约》规定领海宽度不得超过多少海里？</textarea>
          <div class="composer-footer">
            <div class="composer-options">
              <label>研究模式<select id="research-mode"><option value="quick">快速</option><option value="standard" selected>标准</option><option value="deep">深度</option></select></label>
              <label>回答语言<select id="answer-language"><option value="follow_question">跟随问题</option><option value="chinese">简体中文</option><option value="english">English</option></select></label>
            </div>
            <div class="query-actions"><button class="secondary" id="search-button">只检索</button><button class="secondary hidden" id="stop-button">停止</button><button class="primary" id="ask-button"><span>开始研究</span><span aria-hidden="true">↗</span></button></div>
          </div>
          <div id="mode-helper" class="mode-helper"><strong>标准模式</strong><span>适合大多数研究：检索法源并生成带引证的回答。</span><kbd>Ctrl + Enter</kbd></div>
        </div>

        <div class="process-strip" id="process-strip" aria-live="polite">
          <div class="process-step" data-step="retrieve"><span>1</span><div><strong>检索证据</strong><small>FTS5 + BGE-M3</small></div></div><i></i>
          <div class="process-step" data-step="compose"><span>2</span><div><strong>组织回答</strong><small>本地模型</small></div></div><i></i>
          <div class="process-step" data-step="audit"><span>3</span><div><strong>引证核验</strong><small>逐句审计</small></div></div>
        </div>

        <section class="result-sheet">
          <div class="answer-header"><div><div class="section-kicker">研究结论</div><h2 id="result-title">准备就绪</h2></div><div class="grounded-badge hidden" id="grounded-badge">✓ 引证已校验</div></div>
          <div class="empty-state" id="empty-state"><div class="empty-glyph">§</div><h3>提出第一个研究问题</h3><p>你也可以从左侧选择一个研究起点。</p></div>
          <div class="loading-state hidden" id="loading-state"><div class="loader"></div><h3 id="loading-title">正在检索本地资料</h3><p id="loading-copy">正在运行 FTS5 与 BGE-M3 混合检索。</p></div>
          <article class="answer-card hidden" id="answer-card"><div class="answer-warning hidden" id="answer-warning" role="status"></div><div class="answer-copy" id="answer-copy"></div><div id="quick-results" class="quick-results hidden"></div><div class="answer-footer"><div class="answer-meta" id="answer-meta"></div><button class="secondary answer-save" id="save-answer-button">保存到当前项目</button></div></article>
        </section>
        <div class="legal-notice" role="note"><strong>法律免责声明 · 1.1.10</strong><span>ILIA 提供国际法资料检索与辅助解释，不构成法律意见。正式引用及最新法律发展应以官方来源为准。</span></div>
      </section>
    </main>

    <aside class="inspector">
      <div class="inspector-tabs" role="tablist" aria-label="研究检查器">
        <button class="active" type="button" data-inspector="evidence">证据</button>
        <button type="button" data-inspector="source">原文</button>
        <button type="button" data-inspector="audit">审计</button>
      </div>
      <section class="inspector-panel" data-inspector-panel="evidence">
        <div class="evidence-section" id="evidence-section"><div class="section-row"><div><div class="section-kicker">证据包</div><h3>检索证据</h3></div><span id="evidence-count">等待检索</span></div><div class="evidence-list" id="evidence-list"><div class="inspector-empty">开始研究后，入选证据会在这里形成可核验的证据包。</div></div></div>
      </section>
      <section class="inspector-panel hidden" data-inspector-panel="source">
        <div class="source-placeholder" id="source-placeholder"><div class="source-icon">¶</div><h3>原文证据</h3><p>选择一条证据，在这里核对原文、页码、法律性质与官方来源。</p></div>
        <div class="source-detail hidden" id="source-detail">
          <div class="section-kicker">原文证据</div><div class="source-index" id="source-index">证据 1</div>
          <h2 id="source-title"></h2><div class="source-chips" id="source-chips"></div>
          <div class="citation-box"><div>引注预览</div><strong id="source-citation"></strong></div>
          <div class="citation-tools"><select id="citation-style" aria-label="引注格式"><option value="oscola" selected>OSCOLA · 英文论文与脚注</option><option value="chinese">普通中文 · 中文论文与案卷</option><option value="bluebook">Bluebook · 美式法律写作</option><option value="icj">ICJ · 国际法院文件</option><option value="plain_text">纯文本 · Word 与邮件</option><option value="markdown">Markdown · Obsidian 与 GitHub</option></select><button id="copy-citation" class="secondary">复制引注</button></div>
          <div id="citation-help" class="citation-help">OSCOLA 适合英文法律论文和脚注；正式提交前请按文书元数据核对。</div>
          <div class="original-heading"><span>文献内容</span><div id="language-switch" class="language-switch hidden" aria-label="文献语言切换"><button id="original-button" class="active">原文</button><button id="translate-button">译文</button><button id="parallel-button">对照</button></div><span id="source-language"></span></div>
          <div id="translation-note" class="translation-note hidden">本地机器翻译 · 以英文原文为准</div><article id="source-text" class="legal-document" aria-label="法律文书正文"></article><a id="source-link" class="source-link" target="_blank" rel="noreferrer">查看官方来源 ↗</a>
        </div>
      </section>
      <section class="inspector-panel hidden" data-inspector-panel="audit">
        <div class="section-row"><div><div class="section-kicker">逐句审计</div><h3 id="audit-title">等待回答</h3></div><span id="audit-score">—</span></div>
        <div id="audit-list" class="audit-list"><div class="inspector-empty">生成回答后，这里会展示每项实质性陈述与证据的支持关系。</div></div>
      </section>
    </aside>
    <div id="library-modal" class="library-modal hidden" role="dialog" aria-modal="true" aria-labelledby="library-title">
      <div class="library-dialog">
        <div class="library-header"><div><div class="section-kicker">本地资料库</div><h2 id="library-title">浏览规范化法律文本</h2></div><button id="library-close" class="library-close" aria-label="关闭资料库">×</button></div>
        <p>共 49 份国际法资料。文本由 ILIA 从核验基线生成，不是官方排版版本；正式引用请通过文献中的官方来源核验。</p>
        <input id="library-filter" class="library-filter" type="search" placeholder="按中文名、英文名或缩写筛选" />
        <div class="import-panel"><input id="import-path" class="library-filter" placeholder="粘贴本地 PDF / TXT / MD / HTML / DOCX 路径"/><button id="import-button" class="secondary">预览并导入</button><button id="rebuild-button" class="secondary">重建个人索引</button></div>
        <div id="library-count" class="library-count"></div>
        <div id="library-list" class="library-list"></div>
      </div>
    </div>
    <div id="projects-modal" class="library-modal hidden" role="dialog" aria-modal="true" aria-labelledby="projects-title">
      <div class="project-dialog">
        <div class="library-header project-dialog-header"><div><div class="section-kicker">研究工作区</div><h2 id="projects-title">项目、笔记与导出</h2><p>把同一事项的问题、回答、证据与笔记保存在一个项目中。</p></div><div class="dialog-header-actions"><button class="guide-chip" id="project-guide-button" type="button">? 如何使用</button><button id="projects-close" class="library-close" aria-label="关闭项目窗口">×</button></div></div>
        <div class="project-workspace">
          <aside class="project-create-panel">
            <div><div class="section-kicker">新建项目</div><h3>建立研究事项</h3><p>一个项目对应一个案件、专题或长期研究任务。</p></div>
            <div class="project-form">
              <label>项目名称<input id="project-title" class="library-filter" placeholder="例如：南海仲裁研究"/></label>
              <label>受控法律标签<select id="project-tags" class="controlled-tags" multiple aria-describedby="project-tags-help"><option value="文书类型：条约">文书类型 · 条约</option><option value="文书类型：判决">文书类型 · 判决</option><option value="文书类型：咨询意见">文书类型 · 咨询意见</option><option value="法律状态：已生效">法律状态 · 已生效</option><option value="机构：联合国">机构 · 联合国</option><option value="机构：国际法院">机构 · 国际法院</option><option value="机构：国际刑事法院">机构 · 国际刑事法院</option><option value="主题：海洋法">主题 · 海洋法</option><option value="主题：国家责任">主题 · 国家责任</option><option value="主题：条约法">主题 · 条约法</option><option value="主题：人权法">主题 · 人权法</option><option value="程序：管辖权">程序 · 管辖权</option><option value="程序：可受理性">程序 · 可受理性</option><option value="程序：实体">程序 · 实体</option><option value="程序：赔偿">程序 · 赔偿</option></select><small id="project-tags-help">按 Ctrl 可多选；机器代码不会显示给用户。</small></label>
              <label>自定义标签（可选）<input id="project-custom-tags" class="library-filter" placeholder="客户事项，内部阶段"/></label>
              <label>项目说明<textarea id="project-description" rows="4" placeholder="记录研究范围、目标或交付要求"></textarea></label>
              <button id="create-project-button" class="primary">创建并切换到项目</button>
            </div>
          </aside>
          <section class="project-note-panel">
            <div class="project-current-header"><div><div class="section-kicker">当前项目</div><h3 id="project-active-title">未选择项目</h3><p id="project-active-description">请从左侧研究空间选择项目，或新建一个项目。</p><div id="project-active-tags" class="project-tags"></div></div><span id="project-status" class="save-state">等待选择项目</span></div>
            <label class="question-label" for="project-note">研究笔记 · 输入后自动保存</label>
            <textarea id="project-note" rows="14" placeholder="记录争点、事实时间线、待核验问题和结论草稿……"></textarea>
            <div class="project-export-bar"><div><strong>导出研究成果</strong><span>Markdown 适合继续编辑；自包含 HTML 适合归档与分享。</span></div><div class="query-actions"><button id="export-md" class="secondary">导出 Markdown</button><button id="export-html" class="secondary">导出 HTML</button></div></div>
          </section>
        </div>
      </div>
    </div>
    <div id="guide-modal" class="library-modal hidden" role="dialog" aria-modal="true" aria-labelledby="guide-title">
      <div class="guide-dialog">
        <div class="library-header"><div><div class="section-kicker">ILIA Tips</div><h2 id="guide-title">国际法研究工作流指南</h2><p>按工作顺序认识每个区域，不需要一次记住所有功能。</p></div><button id="guide-close" class="library-close" aria-label="关闭使用指南">×</button></div>
        <div class="guide-layout"><nav id="guide-steps" class="guide-steps" aria-label="教程步骤"></nav><section class="guide-content" aria-live="polite"><div id="guide-step-count" class="guide-step-count"></div><div id="guide-illustration" class="guide-illustration"></div><h3 id="guide-step-title"></h3><p id="guide-step-copy"></p><div id="guide-step-tips" class="guide-step-tips"></div></section></div>
        <div class="guide-footer"><span>这个入口会一直保留在左下角。</span><div><button id="guide-back" class="secondary" type="button">上一步</button><button id="guide-next" class="primary" type="button">下一步</button></div></div>
      </div>
    </div>
    <div id="settings-modal" class="library-modal hidden" role="dialog" aria-modal="true" aria-labelledby="settings-title">
      <div class="settings-dialog">
        <div class="library-header settings-header"><div><div class="section-kicker">本机设置</div><h2 id="settings-title">运行环境与数据管理</h2><p>管理本地模型、可信更新来源和研究资料备份。</p></div><button id="settings-close" class="library-close" aria-label="关闭设置">×</button></div>
        <div class="settings-scroll">
          <div class="settings-version-card"><div class="settings-version-mark" aria-hidden="true">i</div><div><span>版本与更新状态</span><strong id="update-report">正在读取版本与更新状态…</strong></div></div>
          <div class="settings-grid">
            <section class="settings-card settings-card-wide" aria-labelledby="performance-title">
              <div class="settings-card-heading"><div><div class="section-kicker">本地推理</div><h3 id="performance-title">性能与资源</h3></div><span class="settings-card-index">01</span></div>
              <p class="settings-card-copy">仅标准与深度研究会启动生成模型；快速检索不会占用模型显存。</p>
              <div class="settings-control-grid">
                <label class="settings-field" for="performance-preset"><span>性能档位</span><select id="performance-preset"><option value="energy_saver">节能</option><option value="balanced" selected>平衡</option><option value="high_performance">高性能</option></select><small>控制上下文长度、GPU 层数与资源占用。</small></label>
                <label class="settings-field" for="idle-timeout"><span>空闲释放显存</span><select id="idle-timeout"><option value="300">5 分钟</option><option value="900" selected>15 分钟</option><option value="1800">30 分钟</option><option value="0">不自动释放</option></select><small>长时间无生成任务时自动结束模型进程。</small></label>
              </div>
              <div class="settings-action-row"><button id="prewarm-button" class="secondary">后台预热模型</button><div id="resource-report" class="settings-inline-status">模型尚未加载；纯检索不会启动 Qwen。</div></div>
            </section>

            <section class="settings-card settings-card-wide" aria-labelledby="update-title">
              <div class="settings-card-heading"><div><div class="section-kicker">安全更新</div><h3 id="update-title">更新来源与离线安装</h3></div><span class="settings-card-index">02</span></div>
              <div class="update-resilience"><strong>签名验证始终启用</strong><span>断流会自动重试和续传；无论使用 GitHub、镜像还是本地包，只有签名与 SHA-256 校验通过后才会替换程序。</span></div>
              <div class="settings-network-grid">
                <div class="settings-field-group"><label class="settings-field" for="mirror-url"><span>中国大陆镜像地址</span><small>可选。镜像不可用时自动回退 GitHub。</small></label><div class="settings-input-action"><input id="mirror-url" class="library-filter" placeholder="https://updates.example.cn/ilia/latest"/><button id="save-mirror" class="secondary">保存</button></div><div id="mirror-status" class="settings-inline-status">未配置正式镜像；将使用 GitHub 官方发布地址。</div></div>
                <div class="settings-field-group"><label class="settings-field" for="proxy-url"><span>更新代理</span><small>支持 HTTP、HTTPS 与 SOCKS5；凭据不会明文显示。</small></label><div class="settings-input-action"><input id="proxy-url" class="library-filter" type="password" autocomplete="off" placeholder="socks5://user:password@127.0.0.1:1080"/><button id="save-proxy" class="secondary">保存</button></div><div id="proxy-status" class="settings-inline-status"></div></div>
              </div>
              <div class="settings-subsection"><div><strong>本地签名更新包</strong><span>适合 GitHub 无法访问或内网部署。选择后先显示版本、组件与体积，再由你确认安装。</span></div></div>
              <div id="local-update-drop" class="local-update-drop" tabindex="0" role="button" aria-describedby="local-update-help"><strong>将 .ilia 文件拖到这里，或点击选择</strong><span id="local-update-help">安装前会验证发布签名、文件哈希和目标版本。</span></div>
              <div class="local-update-picker"><input id="local-update-path" class="library-filter" readonly placeholder="尚未选择更新包"/><button id="local-update-choose" class="secondary" type="button">选择文件</button><button id="local-update-button" class="primary" type="button" disabled>检查并安装</button></div>
            </section>

            <section class="settings-card settings-card-wide" aria-labelledby="backup-title">
              <div class="settings-card-heading"><div><div class="section-kicker">研究数据</div><h3 id="backup-title">备份与恢复</h3></div><span class="settings-card-index">03</span></div>
              <p class="settings-card-copy">备份个人资料库、项目、笔记、会话与证据快照。恢复前会核验文件哈希、SQLite 完整性与架构版本。</p>
              <div class="settings-backup-grid">
                <div class="settings-field-group"><label class="settings-field" for="backup-path"><span>创建备份</span><small>保存为带完整性校验的 .ilia-workspace 文件。</small></label><div class="settings-input-action"><input id="backup-path" class="library-filter" placeholder="例如 D:\\ILIA-backup.ilia-workspace"/><button id="backup-button" class="secondary">创建备份</button></div></div>
                <div class="settings-field-group"><label class="settings-field" for="restore-path"><span>恢复备份</span><small>恢复前不会覆盖当前数据，确认后再执行。</small></label><div class="settings-input-action"><input id="restore-path" class="library-filter" placeholder="粘贴 .ilia-workspace 文件路径"/><button id="restore-button" class="secondary">校验并恢复</button></div></div>
              </div>
              <div id="backup-status" class="settings-inline-status settings-backup-status" role="status" aria-live="polite"></div>
            </section>
          </div>
        </div>
      </div>
    </div>
    <div id="app-toast" class="app-toast hidden" role="status" aria-live="polite"></div>
    <div id="action-modal" class="library-modal hidden" role="dialog" aria-modal="true" aria-labelledby="action-modal-title" aria-describedby="action-modal-message">
      <div class="action-dialog">
        <div id="action-modal-kicker" class="section-kicker">请确认</div>
        <h2 id="action-modal-title">确认操作</h2>
        <p id="action-modal-message"></p>
        <div class="action-dialog-actions"><button id="action-modal-cancel" class="secondary" type="button">取消</button><button id="action-modal-confirm" class="primary" type="button">确认</button></div>
      </div>
    </div>
    <div id="update-notice" class="update-notice hidden" role="status" aria-live="polite"><span id="update-notice-copy"></span><button id="update-notice-close" type="button" aria-label="关闭更新提示">×</button></div>
    <div id="update-progress-modal" class="library-modal hidden" role="dialog" aria-modal="true" aria-labelledby="update-progress-title">
      <div class="update-progress-dialog">
        <div class="section-kicker">安全更新</div><h2 id="update-progress-title">正在准备更新</h2>
        <p id="update-progress-message">ILIA 会先完成下载和校验，在真正替换程序前保持当前窗口打开。</p>
        <div class="update-progress-track" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="0"><span id="update-progress-fill"></span></div>
        <div id="update-progress-detail" class="update-progress-detail">正在连接更新服务器…</div>
        <div class="update-progress-actions"><button id="update-progress-retry" class="primary hidden" type="button">重新连接并续传</button><button id="update-progress-cancel" class="secondary hidden" type="button">取消下载</button><button id="update-progress-close" class="secondary hidden" type="button">关闭</button></div>
      </div>
    </div>
    <div id="reader-modal" class="library-modal hidden" role="dialog" aria-modal="true" aria-labelledby="reader-title">
      <div class="reader-dialog">
        <div class="library-header"><div><div class="section-kicker">ILIA 规范化文本</div><h2 id="reader-title">文献</h2></div><button id="reader-close" class="library-close" aria-label="关闭阅读器">×</button></div>
        <p id="reader-note">非官方排版版本 · 请通过官方来源核验正式引文</p>
        <article id="reader-text" class="reader-text legal-document"></article>
      </div>
    </div>
  </div>`;

const question = document.querySelector<HTMLTextAreaElement>("#question")!;
const askButton = document.querySelector<HTMLButtonElement>("#ask-button")!;
const searchButton = document.querySelector<HTMLButtonElement>("#search-button")!;
const stopButton = document.querySelector<HTMLButtonElement>("#stop-button")!;
const researchMode = document.querySelector<HTMLSelectElement>("#research-mode")!;
const answerLanguage = document.querySelector<HTMLSelectElement>("#answer-language")!;
const updateButton = document.querySelector<HTMLButtonElement>("#update-button")!;
const libraryButton = document.querySelector<HTMLButtonElement>("#library-button")!;
const libraryModal = document.querySelector<HTMLElement>("#library-modal")!;
const libraryClose = document.querySelector<HTMLButtonElement>("#library-close")!;
const libraryFilter = document.querySelector<HTMLInputElement>("#library-filter")!;
const libraryList = document.querySelector<HTMLElement>("#library-list")!;
const readerModal = document.querySelector<HTMLElement>("#reader-modal")!;
const readerClose = document.querySelector<HTMLButtonElement>("#reader-close")!;
const readerTitle = document.querySelector<HTMLElement>("#reader-title")!;
const readerText = document.querySelector<HTMLElement>("#reader-text")!;
const backendSelect = document.querySelector<HTMLSelectElement>("#backend-select")!;
const sourceLink = document.querySelector<HTMLAnchorElement>("#source-link")!;
const translateButton = document.querySelector<HTMLButtonElement>("#translate-button")!;
const originalButton = document.querySelector<HTMLButtonElement>("#original-button")!;
const parallelButton = document.querySelector<HTMLButtonElement>("#parallel-button")!;
const projectsModal = document.querySelector<HTMLElement>("#projects-modal")!;
const guideModal = document.querySelector<HTMLElement>("#guide-modal")!;
const settingsModal = document.querySelector<HTMLElement>("#settings-modal")!;
const projectSelect = document.querySelector<HTMLSelectElement>("#project-select")!;
let currentHits: SearchHit[] = [];
let currentEvidence: EvidenceItem[] = [];
let currentSourceIndex = -1;
const translationCache = new Map<string, TranslationResponse["translation"]>();
let preferredSourceLanguage: "original" | "chinese" | "parallel" = "original";
let libraryDocuments: DocumentSummary[] = [];
let projects: Project[] = [];
let currentResearch: ResearchResponse | null = null;
let noteTimer: number | null = null;
let activeRequestId: string | null = null;
let streamedAnswer = "";
let lastUpdateReleaseId: string | null = null;
let lastLocalUpdatePath: string | null = null;
let guideStepIndex = 0;
let updateCheckGeneration = 0;
let updateCheckActive = false;
let activeUpdateUrls = { manifestUrl: updateManifestUrl, signatureUrl: updateSignatureUrl, label: "GitHub" };
let currentApplicationVersion = "1.1.10";
let updateDownloadSample: { at: number; bytes: number; speed: number } | null = null;
let toastTimer: number | null = null;
let confirmationResolver: ((confirmed: boolean) => void) | null = null;
let confirmationReturnFocus: HTMLElement | null = null;
const inspectorScroll = new Map<string, number>();

const documentTypeLabels: Record<string, string> = {
  treaty: "条约", judgment: "判决", advisory_opinion: "咨询意见", order: "命令",
  resolution: "决议", draft_articles: "条款草案", customary_rule: "习惯国际法规则",
  commentary: "评注", declaration: "宣言", statute: "规约", user_document: "个人资料",
};
const legalStatusLabels: Record<string, string> = {
  in_force_treaty: "已生效", signed_treaty: "已签署", not_in_force: "未生效",
  judgment: "判决", advisory_opinion: "咨询意见", resolution: "决议", historical: "历史文本",
};
const citationHelp: Record<string, string> = {
  oscola: "OSCOLA 适合英文法律论文和脚注；正式提交前请按文书元数据核对。",
  chinese: "普通中文适合中文论文、研究报告和案卷。",
  bluebook: "Bluebook 适合美式法律写作。",
  icj: "ICJ 格式接近国际法院文件中的引注习惯。",
  plain_text: "纯文本适合粘贴到 Word、邮件及不支持富文本的系统。",
  markdown: "Markdown 仅适合 Obsidian、GitHub 等 Markdown 环境，不等同于完整法律引注。",
};

const guideSteps = [
  { title: "从研究问题开始", focus: "query", copy: "在中央输入研究问题，再按任务选择快速、标准或深度模式。快速模式只返回证据；标准模式生成有据回答；深度模式会先拆分子问题。", tips: ["“只检索”适合先找法源，不启动生成模型。", "“开始研究”会依次完成检索、组织回答和引证审计。"] },
  { title: "用项目管理案件与专题", focus: "project", copy: "一个项目可以对应一个案件、客户事项、论文专题或长期跟踪任务。切换项目后，新回答、证据快照和研究笔记会归入该项目。", tips: ["受控标签按文书、机构、主题和程序维度组织。", "笔记输入后自动保存，不需要手动提交。"] },
  { title: "核验证据、原文与引证", focus: "inspector", copy: "右侧检查器是质量控制区：证据显示检索命中，原文用于核对页码和官方来源，审计逐句标明回答是否获得证据支持。", tips: ["点击证据卡即可跳到对应原文。", "支持状态不替代正式法律审查。"] },
  { title: "扩展本地资料库", focus: "library", copy: "资料库包含随软件发布的规范化国际法文本，也可以导入自己的 PDF、TXT、Markdown、HTML 或 DOCX，建立仅存于本机的个人索引。", tips: ["导入前会先显示预览和分块数量。", "正式引用仍应通过官方来源链接复核。"] },
  { title: "沉淀笔记并导出", focus: "export", copy: "项目工作区保存研究笔记和研究记录。完成后可导出 Markdown 继续编辑，或导出自包含 HTML 进行归档和发送。", tips: ["导出前确认左侧已选中正确项目。", "备份功能可同时保存项目、笔记、会话和个人资料库。"] },
  { title: "安全更新与本机设置", focus: "update", copy: "更新检查在后台运行；下载中断会重试和续传，签名与哈希核验通过后才替换程序。", tips: ["检查结果会缓存并显示上次检查时间。", "GitHub 不可达时可配置代理或使用本地签名更新包。"] },
];

function showToast(message: string, tone: "success" | "error" | "info" = "info") {
  const toast = document.querySelector<HTMLElement>("#app-toast")!;
  toast.textContent = message;
  toast.dataset.tone = tone;
  toast.classList.remove("hidden");
  if (toastTimer !== null) window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => toast.classList.add("hidden"), 3200);
}

function errorMessage(error: unknown, fallback = "操作未完成") {
  if (typeof error === "string" && error.trim()) return error.trim();
  if (error instanceof Error && error.message.trim()) return error.message.trim();
  if (error && typeof error === "object") {
    const value = error as Record<string, unknown>;
    const message = [value.message_zh, value.safe_message, value.message, value.error]
      .find((item): item is string => typeof item === "string" && item.trim().length > 0);
    const code = typeof value.code === "string" ? value.code : null;
    if (message) return code ? `${message}（${code}）` : message;
    try {
      const serialized = JSON.stringify(error);
      if (serialized && serialized !== "{}") return serialized;
    } catch { /* 使用安全的兜底文案。 */ }
  }
  return fallback;
}

function closeConfirmation(confirmed: boolean) {
  const modal = document.querySelector<HTMLElement>("#action-modal")!;
  modal.classList.add("hidden");
  const resolve = confirmationResolver;
  confirmationResolver = null;
  resolve?.(confirmed);
  confirmationReturnFocus?.focus();
  confirmationReturnFocus = null;
}

function confirmAction(options: { title: string; message: string; confirmLabel?: string; kicker?: string; danger?: boolean }) {
  if (confirmationResolver) closeConfirmation(false);
  confirmationReturnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const modal = document.querySelector<HTMLElement>("#action-modal")!;
  const confirm = document.querySelector<HTMLButtonElement>("#action-modal-confirm")!;
  document.querySelector<HTMLElement>("#action-modal-kicker")!.textContent = options.kicker ?? "请确认";
  document.querySelector<HTMLElement>("#action-modal-title")!.textContent = options.title;
  document.querySelector<HTMLElement>("#action-modal-message")!.textContent = options.message;
  confirm.textContent = options.confirmLabel ?? "确认";
  confirm.classList.toggle("danger", Boolean(options.danger));
  modal.classList.remove("hidden");
  requestAnimationFrame(() => confirm.focus());
  return new Promise<boolean>((resolve) => { confirmationResolver = resolve; });
}

function legalLineKind(line: string): "title" | "part" | "article" | "paragraph" | "body" {
  const value = line.trim();
  if (/^(part|chapter|section|book|编|章|节)\s+[\divxlcdm一二三四五六七八九十百]+/i.test(value)) return "part";
  if (/^(article\s+\d+[a-z-]*|第[一二三四五六七八九十百千\d]+条)/i.test(value)) return "article";
  if (/^(\(?\d+[.)]|\([a-z]\)|[（(][一二三四五六七八九十]+[）)])\s*/i.test(value)) return "paragraph";
  if (value.length < 100 && /^(convention|agreement|statute|charter|公约|协定|规约|宪章)/i.test(value)) return "title";
  return "body";
}

function renderLegalDocument(target: HTMLElement, text: string, translation?: string) {
  const makeColumn = (value: string, language: string) => {
    const column = document.createElement("section");
    column.className = "legal-column";
    column.lang = language;
    const lines = value.replace(/\r/g, "").split(/\n+/).map((line) => line.trim()).filter(Boolean);
    lines.forEach((line) => {
      const kind = legalLineKind(line);
      const node = document.createElement(kind === "title" ? "h2" : kind === "part" ? "h3" : kind === "article" ? "h4" : "p");
      node.className = `legal-${kind}`;
      node.textContent = line;
      column.append(node);
    });
    return column;
  };
  target.replaceChildren();
  target.classList.toggle("parallel", Boolean(translation));
  target.append(makeColumn(text, "en"));
  if (translation) target.append(makeColumn(translation, "zh-CN"));
}

function openInspector(name: "evidence" | "source" | "audit") {
  const visible = document.querySelector<HTMLElement>("[data-inspector-panel]:not(.hidden)");
  if (visible?.dataset.inspectorPanel) inspectorScroll.set(visible.dataset.inspectorPanel, visible.scrollTop);
  document.querySelectorAll<HTMLButtonElement>("[data-inspector]").forEach((button) => {
    button.classList.toggle("active", button.dataset.inspector === name);
  });
  document.querySelectorAll<HTMLElement>("[data-inspector-panel]").forEach((panel) => {
    panel.classList.toggle("hidden", panel.dataset.inspectorPanel !== name);
  });
  const next = document.querySelector<HTMLElement>(`[data-inspector-panel="${name}"]`);
  if (next) requestAnimationFrame(() => { next.scrollTop = inspectorScroll.get(name) ?? 0; });
}

function setProcess(step: "idle" | "retrieve" | "compose" | "audit" | "done") {
  const quick = researchMode.value === "quick";
  const strip = document.querySelector<HTMLElement>("#process-strip")!;
  strip.classList.toggle("quick-process", quick);
  const order = ["retrieve", "compose", "audit"];
  const current = order.indexOf(step);
  document.querySelectorAll<HTMLElement>(".process-step").forEach((item, index) => {
    item.classList.toggle("done", step === "done" || (current >= 0 && index < current));
    item.classList.toggle("active", current === index);
    const marker = item.querySelector<HTMLElement>(":scope > span");
    if (marker) marker.textContent = quick && index === 0 && (step === "done" || step === "compose") ? "完成" : String(index + 1);
  });
}

function renderAudit(findings: CitationFinding[]) {
  const list = document.querySelector<HTMLElement>("#audit-list")!;
  const supported = findings.filter((finding) => finding.support === "direct" || finding.support === "summary").length;
  document.querySelector<HTMLElement>("#audit-title")!.textContent = findings.length ? `${supported} / ${findings.length} 项通过` : "无可审计陈述";
  document.querySelector<HTMLElement>("#audit-score")!.textContent = findings.length ? `${Math.round((supported / findings.length) * 100)}%` : "—";
  list.replaceChildren();
  if (!findings.length) {
    const empty = document.createElement("div"); empty.className = "inspector-empty"; empty.textContent = "本次结果没有需要逐句审计的生成式陈述。"; list.append(empty); return;
  }
  findings.forEach((finding) => {
    const item = document.createElement("div");
    const safe = finding.support === "direct" || finding.support === "summary";
    item.className = `audit-item${safe ? "" : " warn"}`;
    const status = document.createElement("span"); status.className = "audit-status"; status.setAttribute("aria-hidden", "true");
    const body = document.createElement("div");
    const label = document.createElement("strong"); label.textContent = finding.support === "direct" ? "直接支持" : finding.support === "summary" ? "概括支持" : finding.support === "conflict" ? "证据冲突" : "缺少支持";
    const statement = document.createElement("span"); statement.textContent = finding.statement;
    const evidence = document.createElement("small"); evidence.textContent = finding.evidence_numbers.length ? `对应证据 ${finding.evidence_numbers.map((number) => String(number).padStart(2, "0")).join("、")}` : "没有对应证据";
    body.append(label, statement, evidence); item.append(status, body); list.append(item);
  });
}

function setBusy(busy: boolean, title = "正在检索本地资料", copy = "正在运行 FTS5 与 BGE-M3 混合检索。") {
  askButton.disabled = false; searchButton.disabled = busy;
  stopButton.classList.toggle("hidden", !busy);
  document.querySelector("#empty-state")?.classList.add("hidden");
  document.querySelector("#answer-card")?.classList.add("hidden");
  document.querySelector("#evidence-section")?.classList.add("hidden");
  document.querySelector("#grounded-badge")?.classList.add("hidden");
  document.querySelector("#answer-warning")?.classList.add("hidden");
  document.querySelector("#loading-state")?.classList.toggle("hidden", !busy);
  document.querySelector("#source-detail")?.classList.toggle("muted", busy);
  document.querySelector<HTMLElement>("#loading-title")!.textContent = title;
  document.querySelector<HTMLElement>("#loading-copy")!.textContent = copy;
}

function renderStreamDelta(text: string) {
  streamedAnswer += text;
  document.querySelector("#loading-state")?.classList.add("hidden");
  const card = document.querySelector<HTMLElement>("#answer-card")!;
  card.classList.remove("hidden");
  document.querySelector<HTMLElement>("#result-title")!.textContent = "正在生成有据回答";
  document.querySelector<HTMLElement>("#answer-copy")!.textContent = streamedAnswer;
}

function handleResearchEvent(payload: ResearchEventEnvelope) {
  if (payload.request_id !== activeRequestId) return;
  const event = payload.event;
  if (event.type === "plan_ready") {
    setProcess("compose");
    setBusy(true, "深度研究计划已生成", event.data.subquestions.join(" · "));
  } else if (event.type === "retrieval_completed") {
    setProcess("compose");
    setBusy(true, "证据检索完成", `已选择 ${event.data.evidence_count} 条证据，正在组织回答。`);
  } else if (event.type === "answer_delta") {
    renderStreamDelta(event.data.text);
  } else if (event.type === "answer_replaced") {
    streamedAnswer = event.data.text;
    document.querySelector<HTMLElement>("#answer-copy")!.textContent = streamedAnswer;
    document.querySelector<HTMLElement>("#result-title")!.textContent = "引证复核后已修订";
  } else if (event.type === "citation_audit_completed") {
    setProcess("audit");
    renderAudit(event.data.findings);
    const red = event.data.findings.filter((finding) => finding.support === "unsupported" || finding.support === "conflict").length;
    setBusy(true, "引证复核完成", red ? `${red} 条陈述未获证据支持，已安全处理。` : "全部保留陈述均有对应证据支持。");
  } else if (event.type === "cancelled") {
    setBusy(false);
    document.querySelector<HTMLElement>("#result-title")!.textContent = "已停止";
  } else if (event.type === "failed") {
    showError(event.data.safe_message);
  }
}

function updateRuntime(report: RuntimeProbeReport, active?: RuntimeStartupReport) {
  const selected = active?.selected_backend ?? report.recommended_backend;
  const probe = report.probes.find((item) => item.backend === selected);
  const device = active?.selected_profile.device ?? probe?.recommended_device;
  const label = selected ? selected.toUpperCase() : "不可用";
  document.querySelector<HTMLElement>("#runtime-text")!.textContent = `${label}${device ? ` · ${device}` : ""}`;
  document.querySelector("#runtime-pill")?.classList.toggle("unavailable", !selected);
  backendSelect.value = report.preference;
}

function citationFragment(text: string, citations: AnswerResponse["citations"]): DocumentFragment {
  const fragment = document.createDocumentFragment();
  const expression = /【(\d+)】/g;
  let cursor = 0;
  for (const match of text.matchAll(expression)) {
    fragment.append(document.createTextNode(text.slice(cursor, match.index)));
    const button = document.createElement("button");
    button.className = "citation-token"; button.textContent = match[0];
    const citation = citations.find((item) => item.evidence_number === Number(match[1]));
    button.disabled = !citation;
    if (citation) button.addEventListener("click", () => showSourceByStableKey(citation.stable_key));
    fragment.append(button); cursor = (match.index ?? 0) + match[0].length;
  }
  fragment.append(document.createTextNode(text.slice(cursor)));
  return fragment;
}

function renderSearch(response: SearchResponse, answer?: AnswerResponse, runtime?: RuntimeStartupReport) {
  currentHits = response.hits;
  currentEvidence = response.evidence;
  document.querySelector("#source-detail")?.classList.remove("muted");
  document.querySelector("#loading-state")?.classList.add("hidden");
  const answerCard = document.querySelector<HTMLElement>("#answer-card")!;
  const evidenceSection = document.querySelector<HTMLElement>("#evidence-section")!;
  const answerCopy = document.querySelector<HTMLElement>("#answer-copy")!;
  const quickResults = document.querySelector<HTMLElement>("#quick-results")!;
  const title = document.querySelector<HTMLElement>("#result-title")!;
  if (answer) {
    quickResults.classList.add("hidden");
    title.textContent = answer.citation_rewritten ? "引证复核后的回答" : "有据回答"; answerCopy.replaceChildren(citationFragment(answer.answer, answer.citations)); answerCard.classList.remove("hidden");
    const meta = document.querySelector<HTMLElement>("#answer-meta")!;
    const direct = answer.citation_findings.filter((finding) => finding.support === "direct").length;
    const summary = answer.citation_findings.filter((finding) => finding.support === "summary").length;
    meta.textContent = `${(answer.generation_ms / 1000).toFixed(1)} 秒 · ${runtime?.selected_backend.toUpperCase() ?? "LOCAL"} · ${answer.evidence.length} 条证据 · direct ${direct} / summary ${summary}`;
    document.querySelector("#grounded-badge")?.classList.toggle("hidden", !answer.grounded);
    const warning = document.querySelector<HTMLElement>("#answer-warning")!;
    const red = answer.citation_findings.filter((finding) => finding.support === "unsupported" || finding.support === "conflict").length;
    warning.textContent = red ? `⚠ ${red} 条未获支持的陈述已从最终回答删除。` : "⚠ 回答未通过完整引证校验，请以右侧原文证据为准。";
    warning.classList.toggle("hidden", answer.grounded && red === 0);
    renderAudit(answer.citation_findings);
    setProcess("done");
  } else {
    title.textContent = response.evidence.length ? "检索完成" : "暂无相关资料";
    answerCopy.replaceChildren();
    quickResults.replaceChildren();
    response.evidence.forEach((evidence, index) => {
      const hit = response.hits.find((item) => item.stable_key === evidence.stable_key);
      const card = document.createElement("article"); card.className = "quick-result-card";
      const heading = document.createElement("div"); heading.className = "quick-result-heading";
      const rank = document.createElement("span"); rank.textContent = `证据 ${String(index + 1).padStart(2, "0")}`;
      const relevance = document.createElement("span"); relevance.textContent = hit ? `相关度 ${Math.max(0, Math.min(100, Math.round((1 / (1 + Math.max(hit.score, 0))) * 100)))}%` : "相关证据";
      const cite = document.createElement("strong"); cite.textContent = evidence.citation_label;
      const doc = document.createElement("p"); doc.className = "quick-result-document"; doc.textContent = hit?.title_zh ?? hit?.canonical_title ?? evidence.chunk_id;
      const excerpt = document.createElement("p"); excerpt.textContent = evidence.text.replace(/\s+/g, " ");
      const actions = document.createElement("div"); actions.className = "quick-result-actions";
      const open = document.createElement("button"); open.className = "secondary"; open.textContent = "打开原文"; open.addEventListener("click", () => showSource(index));
      const copy = document.createElement("button"); copy.className = "secondary"; copy.textContent = "复制引注"; copy.addEventListener("click", () => { showSource(index, false); void copyCurrentCitation(); });
      actions.append(open, copy); heading.append(rank, relevance); card.append(heading, cite, doc, excerpt, actions); quickResults.append(card);
    });
    if (!response.evidence.length) {
      const empty = document.createElement("div"); empty.className = "inline-empty"; empty.textContent = "当前资料库没有找到能够直接支持该问题的内容。可调整措辞、移除过滤条件或导入相关资料。"; quickResults.append(empty);
    }
    quickResults.classList.remove("hidden"); answerCard.classList.remove("hidden");
    document.querySelector<HTMLElement>("#answer-meta")!.textContent = response.evidence.length ? `${response.evidence.length} 条入选证据 · 未启动生成模型` : "检索完成 · 未启动生成模型";
    renderAudit([]);
    setProcess("done");
  }
  const list = document.querySelector<HTMLElement>("#evidence-list")!; list.replaceChildren();
  response.evidence.forEach((evidence, index) => {
    const hit = response.hits.find((item) => item.stable_key === evidence.stable_key);
    const card = document.createElement("button"); card.className = "evidence-card";
    const rank = document.createElement("span"); rank.className = "evidence-rank"; rank.textContent = String(index + 1).padStart(2, "0");
    const body = document.createElement("span"); body.className = "evidence-body";
    const cite = document.createElement("strong"); cite.textContent = evidence.citation_label;
    const doc = document.createElement("span"); doc.textContent = hit?.title_zh ?? hit?.canonical_title ?? evidence.chunk_id;
    const excerpt = document.createElement("span"); excerpt.className = "excerpt"; excerpt.textContent = evidence.text.replace(/\s+/g, " ");
    body.append(cite, doc, excerpt); card.append(rank, body); card.addEventListener("click", () => showSource(index)); list.append(card);
  });
  document.querySelector<HTMLElement>("#evidence-count")!.textContent = `${response.evidence.length} 条入选证据`;
  evidenceSection.classList.toggle("hidden", response.evidence.length === 0);
  if (response.evidence.length) showSource(0, false);
  askButton.disabled = false; searchButton.disabled = false;
  stopButton.classList.add("hidden");
}

function showSource(index: number, reveal = true) {
  const cards = Array.from(document.querySelectorAll(".evidence-card")); cards.forEach((card, cardIndex) => card.classList.toggle("active", cardIndex === index));
  const evidence = currentEvidence[index];
  const hit = evidence ? currentHits.find((item) => item.stable_key === evidence.stable_key) : undefined;
  if (!hit) return;
  if (reveal) openInspector("source");
  currentSourceIndex = index;
  document.querySelector("#source-placeholder")?.classList.add("hidden"); document.querySelector("#source-detail")?.classList.remove("hidden");
  document.querySelector<HTMLElement>("#source-index")!.textContent = `证据 ${index + 1}`;
  document.querySelector<HTMLElement>("#source-title")!.textContent = hit.title_zh ?? hit.canonical_title;
  const chips = document.querySelector<HTMLElement>("#source-chips")!; chips.replaceChildren();
  [documentTypeLabels[hit.document_type] ?? hit.document_type, legalStatusLabels[hit.legal_status] ?? hit.legal_status, `第 ${hit.page_start} 页`].forEach((value) => { const chip = document.createElement("span"); chip.textContent = value; chips.append(chip); });
  document.querySelector<HTMLElement>("#source-language")!.textContent = hit.language.toUpperCase();
  const isEnglish = hit.language.toLowerCase().startsWith("en");
  document.querySelector("#language-switch")?.classList.toggle("hidden", !isEnglish);
  if (isEnglish && preferredSourceLanguage !== "original") {
    void translateCurrentSource(false);
  } else {
    showOriginalSource(hit);
  }
  const link = document.querySelector<HTMLAnchorElement>("#source-link")!; link.href = hit.official_source_url;
  void updateCitationPreview();
}

function showSourceByStableKey(stableKey: string) {
  const index = currentEvidence.findIndex((item) => item.stable_key === stableKey);
  if (index >= 0) showSource(index);
}

async function currentCitation(): Promise<string | null> {
  const evidence = currentEvidence[currentSourceIndex];
  const hit = evidence ? currentHits.find((item) => item.stable_key === evidence.stable_key) : undefined;
  if (!hit) return null;
  return call<string>("format_citation", {
    title: hit.canonical_title,
    locator: hit.citation_label,
    url: hit.official_source_url || null,
    style: document.querySelector<HTMLSelectElement>("#citation-style")!.value,
  });
}

async function updateCitationPreview() {
  const select = document.querySelector<HTMLSelectElement>("#citation-style")!;
  document.querySelector<HTMLElement>("#citation-help")!.textContent = citationHelp[select.value] ?? "";
  try {
    const citation = await currentCitation();
    if (citation) document.querySelector<HTMLElement>("#source-citation")!.textContent = citation;
  } catch (error) {
    document.querySelector<HTMLElement>("#source-citation")!.textContent = "暂时无法生成引注预览";
  }
}

async function writeClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const fallback = document.createElement("textarea");
    fallback.value = text;
    fallback.setAttribute("readonly", "");
    fallback.style.position = "fixed";
    fallback.style.opacity = "0";
    document.body.append(fallback);
    fallback.select();
    const copied = document.execCommand("copy");
    fallback.remove();
    if (!copied) throw new Error("系统剪贴板不可用");
  }
}

async function copyCurrentCitation() {
  const select = document.querySelector<HTMLSelectElement>("#citation-style")!;
  const label = select.selectedOptions[0]?.textContent?.split("·")[0].trim() ?? "所选格式";
  try {
    const citation = await currentCitation();
    if (!citation) return showToast("请先选择一条证据。", "info");
    await writeClipboard(citation);
    showToast(`已复制 ${label} 引注`, "success");
  } catch (error) {
    showToast(`复制失败：${errorMessage(error, "系统剪贴板不可用")}`, "error");
  }
}

function showOriginalSource(hit: SearchHit) {
  renderLegalDocument(document.querySelector<HTMLElement>("#source-text")!, hit.text);
  document.querySelector("#translation-note")?.classList.add("hidden");
  originalButton.classList.add("active");
  translateButton.classList.remove("active");
  parallelButton.classList.remove("active");
  translateButton.disabled = false;
  translateButton.textContent = "中文译文";
}

function showChineseTranslation(translation: TranslationResponse["translation"]) {
  renderLegalDocument(document.querySelector<HTMLElement>("#source-text")!, translation.translated_text);
  const note = document.querySelector<HTMLElement>("#translation-note")!;
  note.textContent = `本地机器翻译 · 以英文原文为准 · ${(translation.generation_ms / 1000).toFixed(1)} 秒`;
  note.classList.remove("hidden");
  originalButton.classList.remove("active");
  translateButton.classList.add("active");
  parallelButton.classList.remove("active");
  translateButton.disabled = false;
  translateButton.textContent = "中文译文";
  document.querySelector<HTMLElement>("#source-language")!.textContent = "ZH-CN";
}

async function translateCurrentSource(rememberPreference = true) {
  const evidence = currentEvidence[currentSourceIndex];
  const hit = evidence ? currentHits.find((item) => item.stable_key === evidence.stable_key) : undefined;
  if (!hit) return;
  if (rememberPreference) preferredSourceLanguage = "chinese";
  const cached = translationCache.get(hit.stable_key);
  if (cached) {
    if (preferredSourceLanguage === "parallel") showParallelSource(hit, cached);
    else showChineseTranslation(cached);
    return;
  }
  translateButton.disabled = true;
  translateButton.textContent = "翻译中…";
  document.querySelector<HTMLElement>("#source-text")!.replaceChildren(document.createTextNode("正在生成本地中文译文…"));
  document.querySelector<HTMLElement>("#translation-note")!.textContent = "本地机器翻译 · 以英文原文为准";
  document.querySelector("#translation-note")?.classList.remove("hidden");
  try {
    const response = await call<TranslationResponse>("translate_source", {
      sourceText: hit.text,
      citationLabel: hit.citation_label,
    });
    translationCache.set(hit.stable_key, response.translation);
    updateRuntime(response.runtime.detection, response.runtime);
    const currentEvidenceItem = currentEvidence[currentSourceIndex];
    if (currentEvidenceItem?.stable_key === hit.stable_key) {
      if (preferredSourceLanguage === "parallel") showParallelSource(hit, response.translation);
      else showChineseTranslation(response.translation);
    }
  } catch (error) {
    translateButton.disabled = false;
    translateButton.textContent = "重试翻译";
    showToast(`本地翻译失败：${errorMessage(error)}`, "error");
  }
}

function showParallelSource(hit: SearchHit, translation: TranslationResponse["translation"]) {
  renderLegalDocument(document.querySelector<HTMLElement>("#source-text")!, hit.text, translation.translated_text);
  const note = document.querySelector<HTMLElement>("#translation-note")!;
  note.textContent = "对照阅读 · ILIA 机器翻译，非官方译文 · 复制引文请使用上方原文";
  note.classList.remove("hidden");
  originalButton.classList.remove("active"); translateButton.classList.remove("active"); parallelButton.classList.add("active");
  translateButton.disabled = false;
  translateButton.textContent = "中文译文";
  document.querySelector<HTMLElement>("#source-language")!.textContent = "EN / ZH-CN";
}

function renderLibrary() {
  const query = libraryFilter.value.trim().toLowerCase();
  const visible = libraryDocuments.filter((document) => [
    document.title_zh,
    document.canonical_title,
    document.short_title,
    document.document_type,
  ].filter(Boolean).join(" ").toLowerCase().includes(query));
  document.querySelector<HTMLElement>("#library-count")!.textContent = `显示 ${visible.length} / ${libraryDocuments.length} 份文献`;
  libraryList.replaceChildren();
  for (const document of visible) {
    const item = window.document.createElement("article");
    item.className = "library-item";
    const title = window.document.createElement("strong");
    title.textContent = document.title_zh ?? document.canonical_title;
    const english = window.document.createElement("span");
    english.textContent = document.canonical_title;
    const meta = window.document.createElement("small");
    meta.textContent = `${document.library_kind === "core" ? "核心资料" : "个人资料"} · ${document.document_type} · ${document.legal_status}`;
    const open = window.document.createElement("button");
    open.textContent = "阅读正文";
    open.addEventListener("click", async () => {
      open.disabled = true;
      try {
        readerTitle.textContent = document.title_zh ?? document.canonical_title;
        renderLegalDocument(readerText, await call<string>("read_document_text", { documentId: document.document_id }));
        readerModal.classList.remove("hidden");
      }
      catch (error) { showToast(`无法读取规范化文本：${errorMessage(error)}`, "error"); }
      finally { open.disabled = false; }
    });
    const actions = window.document.createElement("div"); actions.className = "library-actions"; actions.append(open);
    if (document.library_kind === "core") {
      const related = window.document.createElement("button"); related.textContent = "相关资料";
      related.addEventListener("click", async () => {
        const items = await call<DocumentSummary[]>("related_documents", { documentId: document.document_id });
        if (!items.length) {
          related.disabled = true;
          related.textContent = "暂无关联资料";
          showToast("该文献暂无人工维护的关联资料。", "info");
          return;
        }
        const names = items.map((item) => item.title_zh ?? item.canonical_title).join("、");
        showToast(`关联资料：${names}`, "info");
      }); actions.append(related);
    }
    if (document.library_kind === "user") {
      const remove = window.document.createElement("button"); remove.textContent = "删除";
      remove.addEventListener("click", async () => {
        const confirmed = await confirmAction({ title: "删除个人资料", message: `将删除“${document.canonical_title}”及其全部本地索引。此操作不会影响核心资料库。`, confirmLabel: "删除资料", kicker: "个人资料库", danger: true });
        if (!confirmed) return;
        try {
          await call<boolean>("delete_user_document", { documentId: document.document_id });
          libraryDocuments = [];
          await openLibrary(true);
          showToast("个人资料及索引已删除。", "success");
        } catch (error) { showToast(`删除失败：${errorMessage(error)}`, "error"); }
      });
      actions.append(remove);
    }
    item.append(title, english, meta, actions);
    libraryList.append(item);
  }
}

async function openLibrary(force = false) {
  libraryModal.classList.remove("hidden");
  libraryFilter.focus();
  if (libraryDocuments.length && !force) return;
  libraryList.textContent = "正在载入资料目录…";
  try {
    libraryDocuments = await call<DocumentSummary[]>("list_documents");
    renderLibrary();
  } catch (error) {
    libraryList.textContent = `资料库载入失败：${errorMessage(error)}`;
  }
}

async function importDocument() {
  const input = document.querySelector<HTMLInputElement>("#import-path")!;
  if (!input.value.trim()) return input.focus();
  const button = document.querySelector<HTMLButtonElement>("#import-button")!; button.disabled = true; button.textContent = "正在提取…";
  try {
    const preview = await call<ImportPreview>("prepare_import", { path: input.value.trim() });
    const confirmed = await confirmAction({ title: "导入并建立本地索引", message: `${preview.source_filename}\n${preview.chunk_count} 个文本块 · ${(preview.byte_length / 1024).toFixed(1)} KiB\nSHA-256 ${preview.source_sha256.slice(0, 16)}…\n\n${preview.text_preview.slice(0, 700)}`, confirmLabel: "导入并生成向量", kicker: "导入预览" });
    if (!confirmed) { await call("cancel_import", { previewId: preview.preview_id }); return; }
    button.textContent = "正在本地嵌入…";
    const saved = await call<ImportedDocument>("commit_import", { previewId: preview.preview_id, title: preview.inferred_title, language: preview.inferred_language, documentType: preview.inferred_document_type });
    showToast(`已导入“${saved.title}”，共 ${saved.chunk_count} 个文本块。`, "success"); input.value = ""; libraryDocuments = []; await openLibrary(true);
  } catch (error) { showToast(`导入失败：${errorMessage(error)}`, "error"); }
  finally { button.disabled = false; button.textContent = "预览并导入"; }
}

async function loadProjects() {
  projects = await call<Project[]>("list_projects");
  const selected = localStorage.getItem("ilia.project") ?? "";
  projectSelect.replaceChildren(new Option("未选择", ""), ...projects.map((project) => new Option(`${project.title}${project.tags.length ? ` · ${project.tags.join("/")}` : ""}`, project.id)));
  projectSelect.value = projects.some((project) => project.id === selected) ? selected : "";
  const activeProject = projects.find((project) => project.id === projectSelect.value);
  document.querySelector<HTMLElement>("#workspace-context")!.textContent = activeProject?.title ?? "新研究";
  syncProjectWorkspaceContext();
  await loadProjectNote();
}

function syncProjectWorkspaceContext() {
  const activeProject = projects.find((project) => project.id === projectSelect.value);
  document.querySelector<HTMLElement>("#project-active-title")!.textContent = activeProject?.title ?? "未选择项目";
  document.querySelector<HTMLElement>("#project-active-description")!.textContent = activeProject?.description || (activeProject ? "这个项目还没有说明。" : "请从左侧研究空间选择项目，或新建一个项目。");
  const tags = document.querySelector<HTMLElement>("#project-active-tags")!;
  tags.replaceChildren(...(activeProject?.tags ?? []).map((value) => { const tag = document.createElement("span"); tag.textContent = value; return tag; }));
  document.querySelector<HTMLElement>("#project-status")!.textContent = activeProject ? "笔记会自动保存" : "等待选择项目";
}

async function openProjects() {
  projectsModal.classList.remove("hidden");
  try { await loadProjects(); }
  catch (error) { document.querySelector<HTMLElement>("#project-status")!.textContent = `项目读取失败：${errorMessage(error)}`; }
}

let noteLoadGeneration = 0;
let noteSaveChain: Promise<void> = Promise.resolve();
const projectNoteIds = new Map<string, string | null>();
const projectNoteDrafts = new Map<string, string>();
async function loadProjectNote() {
  const note = document.querySelector<HTMLTextAreaElement>("#project-note")!;
  const projectId = projectSelect.value;
  const generation = ++noteLoadGeneration;
  note.value = projectNoteDrafts.get(projectId) ?? ""; note.disabled = !projectSelect.value;
  if (!projectId) return;
  const snapshot = await call<{ notes: Array<{ id: string; body: string }> }>("get_project", { projectId });
  if (generation !== noteLoadGeneration || projectSelect.value !== projectId) return;
  const latest = snapshot.notes.at(-1);
  projectNoteIds.set(projectId, latest?.id ?? null);
  if (!projectNoteDrafts.has(projectId)) note.value = latest?.body ?? "";
  document.querySelector<HTMLElement>("#project-status")!.textContent = projectNoteDrafts.has(projectId) ? "有尚未写入的编辑内容" : "笔记已载入";
}

async function createProject() {
  const title = document.querySelector<HTMLInputElement>("#project-title")!; if (!title.value.trim()) return title.focus();
  const description = document.querySelector<HTMLTextAreaElement>("#project-description")!;
  const tags = document.querySelector<HTMLSelectElement>("#project-tags")!;
  const custom = document.querySelector<HTMLInputElement>("#project-custom-tags")!;
  const controlled = Array.from(tags.selectedOptions, (option) => option.value);
  const customTags = custom.value.split(/[,，]/).map((value) => `自定义：${value.trim()}`).filter((value) => value !== "自定义：");
  const project = await call<Project>("create_project", { title: title.value.trim(), description: description.value, tags: [...controlled, ...customTags] });
  localStorage.setItem("ilia.project", project.id); title.value = ""; description.value = ""; Array.from(tags.options).forEach((option) => { option.selected = false; }); custom.value = ""; await loadProjects();
  document.querySelector<HTMLElement>("#project-status")!.textContent = `已创建并选中“${project.title}”。后续研究将自动保存。`;
}

function renderGuideStep() {
  const step = guideSteps[guideStepIndex];
  document.querySelector<HTMLElement>("#guide-step-count")!.textContent = `第 ${guideStepIndex + 1} 步 · 共 ${guideSteps.length} 步`;
  const preview = document.querySelector<HTMLElement>("#guide-illustration")!;
  preview.innerHTML = `<div class="guide-ui" data-focus="${step.focus}">
    <aside class="guide-ui-rail"><b>ILIA</b><span>研究</span><span>项目</span><span data-guide-target="library">资料库</span><i></i><span data-guide-target="update">检查更新</span></aside>
    <aside class="guide-ui-project" data-guide-target="project"><b>研究空间</b><small>当前项目</small><strong>南海仲裁</strong><p>领海宽度与基线</p><p>条约解释方法</p></aside>
    <section class="guide-ui-main"><div class="guide-ui-query" data-guide-target="query"><small>研究问题</small><strong>《联合国海洋法公约》如何规定领海宽度？</strong><div><span>标准模式</span><button>开始研究</button></div></div><article class="guide-ui-answer"><small>研究结论</small><strong>领海宽度不得超过十二海里</strong><p>《联合国海洋法公约》第 3 条规定，各国有权确定其领海宽度……</p><button data-guide-target="export">保存并导出</button></article></section>
    <aside class="guide-ui-inspector" data-guide-target="inspector"><b>证据　原文　审计</b><strong>UNCLOS · 第 3 条</strong><p>Every State has the right to establish the breadth…</p><span>直接支持</span></aside>
  </div>`;
  document.querySelector<HTMLElement>("#guide-step-title")!.textContent = step.title;
  document.querySelector<HTMLElement>("#guide-step-copy")!.textContent = step.copy;
  const tips = document.querySelector<HTMLElement>("#guide-step-tips")!;
  tips.replaceChildren(...step.tips.map((copy) => { const item = document.createElement("div"); item.textContent = copy; return item; }));
  document.querySelectorAll<HTMLButtonElement>(".guide-step-button").forEach((button, index) => button.classList.toggle("active", index === guideStepIndex));
  document.querySelector<HTMLButtonElement>("#guide-back")!.disabled = guideStepIndex === 0;
  document.querySelector<HTMLButtonElement>("#guide-next")!.textContent = guideStepIndex === guideSteps.length - 1 ? "完成" : "下一步";
}

function openGuide(step = 0) {
  guideStepIndex = Math.max(0, Math.min(step, guideSteps.length - 1));
  const navigation = document.querySelector<HTMLElement>("#guide-steps")!;
  if (!navigation.childElementCount) {
    navigation.replaceChildren(...guideSteps.map((item, index) => {
      const button = document.createElement("button"); button.type = "button"; button.className = "guide-step-button";
      const marker = document.createElement("span"); marker.textContent = String(index + 1).padStart(2, "0");
      const label = document.createElement("strong"); label.textContent = item.title;
      button.append(marker, label); button.addEventListener("click", () => { guideStepIndex = index; renderGuideStep(); }); return button;
    }));
  }
  renderGuideStep();
  guideModal.classList.remove("hidden");
}

function updateModeHelper() {
  const descriptions: Record<ResearchMode, [string, string]> = {
    quick: ["快速模式", "只检索并列出相关法源，不启动生成模型，速度最快。"],
    standard: ["标准模式", "适合大多数研究：检索法源并生成带引证的回答。"],
    deep: ["深度模式", "适合复杂争点：先拆分子问题，再综合多组证据。"],
  };
  const [title, copy] = descriptions[researchMode.value as ResearchMode];
  const helper = document.querySelector<HTMLElement>("#mode-helper")!;
  helper.querySelector("strong")!.textContent = title;
  helper.querySelector("span")!.textContent = copy;
}

async function saveResearchToProject(silent = false) {
  if (!projectSelect.value || !currentResearch?.answer) { if (!silent) showToast("请先选择项目并生成回答。", "info"); return; }
  const evidence = currentResearch.search.evidence.map((item) => { const hit = currentResearch!.search.hits.find((value) => value.stable_key === item.stable_key); return { stable_evidence_key: item.stable_key, title: hit?.title_zh ?? hit?.canonical_title ?? item.chunk_id, citation_label: item.citation_label, text_snapshot: item.text }; });
  try { await call("save_research", { request: { project_id: projectSelect.value, conversation_title: question.value.trim().slice(0, 80), question: question.value.trim(), answer: currentResearch.answer.answer, evidence } }); if (!silent) showToast("回答、会话与证据快照已保存。", "success"); }
  catch (error) { showToast(`自动保存失败，当前回答仍保留在页面：${errorMessage(error)}`, "error"); }
}

function downloadExport(content: string, extension: "md" | "html") {
  const project = projects.find((value) => value.id === projectSelect.value); const blob = new Blob([content], { type: extension === "html" ? "text/html;charset=utf-8" : "text/markdown;charset=utf-8" });
  const url = URL.createObjectURL(blob); const link = document.createElement("a"); link.href = url; link.download = `${project?.title ?? "ILIA-project"}.${extension}`; link.click(); URL.revokeObjectURL(url);
}

async function exportCurrent(format: "markdown" | "html") { if (!projectSelect.value) return showToast("请先选择项目。", "info"); const content = await call<string>("export_project", { projectId: projectSelect.value, format }); downloadExport(content, format === "html" ? "html" : "md"); }

async function openSettings() {
  settingsModal.classList.remove("hidden");
  const mirror = localStorage.getItem("ilia.update-mirror") ?? "";
  document.querySelector<HTMLInputElement>("#mirror-url")!.value = mirror;
  document.querySelector<HTMLElement>("#mirror-status")!.textContent = mirror ? `优先使用：${mirror}；失败时回退 GitHub。签名与 SHA-256 校验不会跳过。` : "未配置正式镜像；将使用 GitHub 官方发布地址。";
  const settings = await call<ProxySettings>("get_proxy_settings");
  const proxySource = { manual: "手动设置", environment: "系统环境", system: "Windows 系统代理" }[settings.source ?? "manual"];
  document.querySelector<HTMLElement>("#proxy-status")!.textContent = settings.enabled ? `已启用（${proxySource}）：${settings.redacted_url}` : "未检测到代理；在线更新将直接连接，仍可使用本地签名更新包。";
  await refreshUpdateSummary(false);
}

async function refreshUpdateSummary(showNotice: boolean) {
  const summary = await call<UpdateSummary>("get_update_summary");
  currentApplicationVersion = summary.current_version;
  const release = summary.last_release_id ?? "尚无更新记录";
  const statusLabels: Record<string, string> = { staged: "已下载", applying: "安装中", applied: "安装成功", rolling_back: "正在回滚", rolled_back: "已回滚", failed: "安装失败" };
  const checked = localStorage.getItem("ilia.update-last-checked");
  const checkedLabel = checked ? ` · 上次检查 ${new Date(checked).toLocaleString()}` : " · 尚未检查更新";
  document.querySelector<HTMLElement>("#update-report")!.textContent = `当前版本 ${summary.current_version} · 最近更新 ${release}${summary.last_status ? ` · ${statusLabels[summary.last_status]}` : ""}${checkedLabel}`;
  if (!showNotice || !summary.last_release_id || !summary.last_status) return;
  const noticeKey = `${summary.last_release_id}:${summary.last_status}`;
  if (localStorage.getItem("ilia.update-notice") === noticeKey) return;
  const notice = document.querySelector<HTMLElement>("#update-notice")!;
  const copy = document.querySelector<HTMLElement>("#update-notice-copy")!;
  if (summary.last_status === "applied") copy.textContent = `ILIA 已更新至 ${summary.current_version}，更新组件已验证并安装。`;
  else if (summary.last_status === "rolled_back") copy.textContent = `${summary.last_release_id} 安装未完成，ILIA 已自动回滚到可用版本。`;
  else if (summary.last_status === "failed") copy.textContent = `${summary.last_release_id} 安装失败，请在设置中重试或使用本地签名更新包。`;
  else return;
  localStorage.setItem("ilia.update-notice", noticeKey);
  notice.classList.remove("hidden");
}

async function runSearch() {
  if (!question.value.trim()) return question.focus();
  setProcess("retrieve");
  openInspector("evidence");
  setBusy(true);
  try { renderSearch(await call<SearchResponse>("search_documents", { query: question.value.trim() })); }
  catch (error) { showError(error); }
}

async function runAsk() {
  if (!question.value.trim()) return question.focus();
  const requestId = crypto.randomUUID();
  activeRequestId = requestId;
  streamedAnswer = "";
  const mode = researchMode.value as ResearchMode;
  setProcess("retrieve");
  openInspector("evidence");
  setBusy(
    true,
    mode === "deep" ? "正在制定深度研究计划" : "正在检索本地资料",
    mode === "quick" ? "快速查询不会启动生成模型。" : "完成证据检索后，将由 Qwen3-4B 流式组织回答。",
  );
  try {
    const response = await call<ResearchResponse>("start_research", {
      request: {
        request_id: requestId,
        question: question.value.trim(),
        mode,
        answer_language: answerLanguage.value as AnswerLanguage,
        filters: { library: "all", document_types: [], topic_ids: [], document_keys: [] },
      },
    });
    if (activeRequestId !== requestId) return;
    if (response.runtime) updateRuntime(response.runtime.detection, response.runtime);
    renderSearch(response.search, response.answer ?? undefined, response.runtime ?? undefined);
    currentResearch = response;
    if (response.answer && projectSelect.value) await saveResearchToProject(true);
    activeRequestId = null;
  } catch (error) {
    if (activeRequestId !== requestId) return;
    activeRequestId = null;
    if (errorMessage(error).toLowerCase().includes("cancel")) {
      setBusy(false);
      document.querySelector<HTMLElement>("#result-title")!.textContent = "已停止";
      return;
    }
    showError(error);
  }
}

async function stopResearch() {
  if (!activeRequestId) return;
  stopButton.disabled = true;
  try { await call<boolean>("cancel_research", { requestId: activeRequestId }); }
  finally { stopButton.disabled = false; }
}

function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB"];
  const index = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  return `${(bytes / 1024 ** index).toFixed(index > 1 ? 1 : 0)} ${units[index]}`;
}

function updateComponentLabel(component: UpdateComponent) {
  const labels: Record<UpdateComponent["kind"], string> = { application: "应用程序", corpus: "资料库", model: "模型", runtime: "运行时" };
  return `${labels[component.kind]} ${component.version} · ${formatBytes(component.payload_size)}`;
}

function setLocalUpdatePackage(path: string) {
  const input = document.querySelector<HTMLInputElement>("#local-update-path")!;
  const install = document.querySelector<HTMLButtonElement>("#local-update-button")!;
  const drop = document.querySelector<HTMLElement>("#local-update-drop")!;
  if (!path.toLowerCase().endsWith(".ilia")) {
    showToast("请选择扩展名为 .ilia 的签名更新包。", "error");
    return false;
  }
  input.value = path;
  install.disabled = false;
  const filename = path.split(/[\\/]/).pop() ?? path;
  drop.querySelector("strong")!.textContent = `已选择：${filename}`;
  return true;
}

async function chooseLocalUpdatePackage() {
  if (!isTauri()) return showToast("文件选择器仅在桌面应用中可用。", "info");
  const selected = await openFileDialog({ multiple: false, directory: false, filters: [{ name: "ILIA 签名更新包", extensions: ["ilia"] }] });
  if (typeof selected === "string") setLocalUpdatePackage(selected);
}

function configuredUpdateSources() {
  const mirror = localStorage.getItem("ilia.update-mirror")?.replace(/\/+$/, "");
  const sources = [] as Array<{ manifestUrl: string; signatureUrl: string; label: string }>;
  if (mirror) sources.push({ manifestUrl: `${mirror}/update-manifest.json`, signatureUrl: `${mirror}/update-manifest.sig`, label: "中国大陆镜像" });
  sources.push({ manifestUrl: updateManifestUrl, signatureUrl: updateSignatureUrl, label: "GitHub" });
  return sources;
}

async function fetchUpdateStatus(generation: number) {
  let lastError: unknown = new Error("没有可用的更新源");
  for (const source of configuredUpdateSources()) {
    if (generation !== updateCheckGeneration) throw new Error("更新检查已取消");
    try {
      const status = await Promise.race([
        call<UpdateStatus>("check_updates", { manifestUrl: source.manifestUrl, signatureUrl: source.signatureUrl }),
        new Promise<never>((_, reject) => window.setTimeout(() => reject(new Error(`${source.label} 连接超时`)), 12_000)),
      ]);
      activeUpdateUrls = source;
      return status;
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError;
}

async function checkForUpdates(background = false) {
  const label = document.querySelector<HTMLElement>("#update-button-label")!;
  const original = label.textContent;
  if (updateCheckActive) {
    updateCheckGeneration += 1;
    updateCheckActive = false;
    label.textContent = original;
    showToast("已取消等待更新检查；后台请求不会影响研究。", "info");
    return;
  }
  const generation = ++updateCheckGeneration;
  updateCheckActive = true;
  label.textContent = "取消检查";
  try {
    const status = await fetchUpdateStatus(generation);
    if (generation !== updateCheckGeneration) return;
    localStorage.setItem("ilia.update-last-checked", new Date().toISOString());
    const available = status.manifest.components.filter(
      (component) => status.installed_versions.components[component.id] !== component.version,
    );
    if (!available.length) {
      if (!background) showToast("当前已经是最新版本。", "success");
      return;
    }
    const totalBytes = available.reduce((sum, component) => sum + component.payload_size, 0);
    const summary = available.map((component) => `• ${updateComponentLabel(component)}`).join("\n");
    if (background) {
      const notice = document.querySelector<HTMLElement>("#update-notice")!;
      document.querySelector<HTMLElement>("#update-notice-copy")!.textContent = `发现 ${status.manifest.release_id} 更新。点击“检查更新”查看并安装。`;
      notice.classList.remove("hidden");
      return;
    }
    const confirmed = await confirmAction({
      title: `安装 ${status.manifest.release_id}`,
      message: `当前版本：${currentApplicationVersion}\n目标版本：${status.manifest.release_id.replace(/^v/, "")}\n更新来源：${activeUpdateUrls.label}\n总下载量：${formatBytes(totalBytes)}\n\n${summary}\n\nILIA 会先下载并验证签名、大小和 SHA-256。校验完成后应用将短暂退出并自动重新打开。`,
      confirmLabel: "下载并安装",
      kicker: "安全更新",
    });
    if (!confirmed) return;
    label.textContent = "准备更新…";
    lastUpdateReleaseId = status.manifest.release_id;
    lastLocalUpdatePath = null;
    updateDownloadSample = null;
    showUpdateProgress({ phase: "checking", release_id: status.manifest.release_id, component_id: null, downloaded_bytes: 0, total_bytes: 0, message_zh: "正在启动安全更新进程" });
    await call<void>("install_update", {
      manifestUrl: activeUpdateUrls.manifestUrl,
      signatureUrl: activeUpdateUrls.signatureUrl,
    });
  } catch (error) {
    if (generation !== updateCheckGeneration) return;
    if (!document.querySelector("#update-progress-modal")?.classList.contains("hidden")) {
      showUpdateProgress({ phase: "failed", release_id: "", component_id: null, downloaded_bytes: 0, total_bytes: 0, message_zh: errorMessage(error, "无法连接更新服务器") });
    }
    if (!background) showToast(`更新检查失败：${errorMessage(error, "无法连接更新服务器")}`, "error");
  } finally {
    if (generation === updateCheckGeneration) {
      updateCheckActive = false;
      label.textContent = original;
    }
  }
}

async function retryUpdate() {
  if (!lastUpdateReleaseId) return;
  const retry = document.querySelector<HTMLButtonElement>("#update-progress-retry")!;
  retry.disabled = true;
  showUpdateProgress({ phase: "checking", release_id: lastUpdateReleaseId, component_id: null, downloaded_bytes: 0, total_bytes: 0, message_zh: "正在重新连接；已下载的有效片段会继续使用" });
  try {
    if (lastLocalUpdatePath) await call<void>("install_local_update", { packagePath: lastLocalUpdatePath });
    else await call<void>("install_update", { manifestUrl: activeUpdateUrls.manifestUrl, signatureUrl: activeUpdateUrls.signatureUrl });
  } catch (error) {
    showUpdateProgress({ phase: "failed", release_id: lastUpdateReleaseId, component_id: null, downloaded_bytes: 0, total_bytes: 0, message_zh: errorMessage(error, "更新下载未完成") });
  } finally {
    retry.disabled = false;
  }
}

async function cancelUpdateDownload() {
  const button = document.querySelector<HTMLButtonElement>("#update-progress-cancel")!;
  button.disabled = true;
  button.textContent = "正在取消…";
  try {
    const cancelled = await call<boolean>("cancel_update");
    if (!cancelled) showToast("当前没有可取消的更新下载。", "info");
  } catch (error) {
    showToast(`无法取消更新：${errorMessage(error)}`, "error");
  } finally {
    button.disabled = false;
    button.textContent = "取消下载";
  }
}

function showUpdateProgress(progress: UpdateProgress) {
  const modal = document.querySelector<HTMLElement>("#update-progress-modal")!;
  const title = document.querySelector<HTMLElement>("#update-progress-title")!;
  const message = document.querySelector<HTMLElement>("#update-progress-message")!;
  const detail = document.querySelector<HTMLElement>("#update-progress-detail")!;
  const fill = document.querySelector<HTMLElement>("#update-progress-fill")!;
  const track = fill.parentElement!;
  const close = document.querySelector<HTMLButtonElement>("#update-progress-close")!;
  const retry = document.querySelector<HTMLButtonElement>("#update-progress-retry")!;
  const cancel = document.querySelector<HTMLButtonElement>("#update-progress-cancel")!;
  modal.classList.remove("hidden");
  const percent = progress.total_bytes > 0 ? Math.min(100, Math.round(progress.downloaded_bytes / progress.total_bytes * 100)) : 0;
  fill.style.width = `${percent}%`;
  track.setAttribute("aria-valuenow", String(percent));
  close.classList.toggle("hidden", progress.phase !== "failed" && progress.phase !== "cancelled");
  retry.classList.toggle("hidden", (progress.phase !== "failed" && progress.phase !== "cancelled") || !lastUpdateReleaseId);
  cancel.classList.toggle("hidden", !["checking", "downloading", "verifying"].includes(progress.phase));
  if (progress.phase === "downloading") {
    const now = performance.now();
    if (updateDownloadSample && progress.downloaded_bytes >= updateDownloadSample.bytes) {
      const elapsed = Math.max(0.2, (now - updateDownloadSample.at) / 1000);
      const instant = (progress.downloaded_bytes - updateDownloadSample.bytes) / elapsed;
      updateDownloadSample.speed = updateDownloadSample.speed > 0 ? updateDownloadSample.speed * .72 + instant * .28 : instant;
      updateDownloadSample.at = now;
      updateDownloadSample.bytes = progress.downloaded_bytes;
    } else {
      updateDownloadSample = { at: now, bytes: progress.downloaded_bytes, speed: 0 };
    }
    const speed = updateDownloadSample.speed;
    const remaining = speed > 0 ? Math.max(0, (progress.total_bytes - progress.downloaded_bytes) / speed) : 0;
    const timing = speed > 0 ? ` · ${formatBytes(speed)}/s · 约 ${remaining < 60 ? `${Math.ceil(remaining)} 秒` : `${Math.ceil(remaining / 60)} 分钟`}` : "";
    title.textContent = `正在下载更新 · ${percent}%`;
    message.textContent = "网络波动会自动重试并从已下载位置续传；下载和校验完成前 ILIA 不会退出。";
    detail.textContent = `${progress.message_zh} · ${formatBytes(progress.downloaded_bytes)} / ${formatBytes(progress.total_bytes)}${timing}`;
  } else if (progress.phase === "verifying") {
    title.textContent = "正在验证更新"; message.textContent = "正在核对签名、大小和 SHA-256。"; detail.textContent = progress.message_zh;
  } else if (progress.phase === "ready_to_apply") {
    title.textContent = "下载与验证完成"; message.textContent = "ILIA 即将退出以替换程序，完成后会自动重新打开。"; detail.textContent = progress.message_zh; fill.style.width = "100%"; track.setAttribute("aria-valuenow", "100");
  } else if (progress.phase === "failed") {
    title.textContent = "更新未完成"; message.textContent = "ILIA 保持打开，现有版本没有被替换。已下载的有效片段会保留供续传。"; detail.textContent = progress.message_zh;
  } else if (progress.phase === "cancelled") {
    title.textContent = "更新已取消"; message.textContent = "当前版本保持不变；已下载的有效片段会保留，下次可以继续。"; detail.textContent = progress.message_zh;
  } else {
    title.textContent = "正在准备更新"; message.textContent = "ILIA 会先完成下载和校验，在真正替换程序前保持当前窗口打开。"; detail.textContent = progress.message_zh;
  }
}

function showError(error: unknown) {
  setBusy(false); document.querySelector<HTMLElement>("#result-title")!.textContent = "暂时无法完成";
  const empty = document.querySelector<HTMLElement>("#empty-state")!; empty.classList.remove("hidden");
  empty.querySelector("h3")!.textContent = "本地服务发生错误"; empty.querySelector("p")!.textContent = errorMessage(error);
  askButton.disabled = false; searchButton.disabled = false;
}

askButton.addEventListener("click", runAsk); searchButton.addEventListener("click", runSearch);
stopButton.addEventListener("click", stopResearch);
updateButton.addEventListener("click", () => { void checkForUpdates(false); });
libraryButton.addEventListener("click", () => { void openLibrary(); });
document.querySelector("#projects-button")?.addEventListener("click", () => { void openProjects(); });
document.querySelector("#project-manage-shortcut")?.addEventListener("click", () => { void openProjects(); });
document.querySelector("#export-current-shortcut")?.addEventListener("click", () => { void openProjects(); });
document.querySelector("#guide-button")?.addEventListener("click", () => openGuide());
document.querySelector("#project-guide-button")?.addEventListener("click", () => openGuide(1));
document.querySelector("#settings-button")?.addEventListener("click", () => { void openSettings(); });
document.querySelectorAll<HTMLButtonElement>("[data-inspector]").forEach((button) => button.addEventListener("click", () => openInspector(button.dataset.inspector as "evidence" | "source" | "audit")));
libraryClose.addEventListener("click", () => libraryModal.classList.add("hidden"));
document.querySelector("#projects-close")?.addEventListener("click", () => projectsModal.classList.add("hidden"));
document.querySelector("#guide-close")?.addEventListener("click", () => guideModal.classList.add("hidden"));
document.querySelector("#guide-back")?.addEventListener("click", () => { if (guideStepIndex > 0) { guideStepIndex -= 1; renderGuideStep(); } });
document.querySelector("#guide-next")?.addEventListener("click", () => { if (guideStepIndex < guideSteps.length - 1) { guideStepIndex += 1; renderGuideStep(); } else { localStorage.setItem("ilia.guide-completed", "true"); guideModal.classList.add("hidden"); document.querySelector("#first-run-tip")?.classList.add("hidden"); } });
document.querySelector("#settings-close")?.addEventListener("click", () => settingsModal.classList.add("hidden"));
document.querySelector("#update-notice-close")?.addEventListener("click", () => document.querySelector("#update-notice")?.classList.add("hidden"));
document.querySelector("#update-progress-close")?.addEventListener("click", () => document.querySelector("#update-progress-modal")?.classList.add("hidden"));
document.querySelector("#update-progress-retry")?.addEventListener("click", () => { void retryUpdate(); });
document.querySelector("#update-progress-cancel")?.addEventListener("click", () => { void cancelUpdateDownload(); });
document.querySelector("#action-modal-cancel")?.addEventListener("click", () => closeConfirmation(false));
document.querySelector("#action-modal-confirm")?.addEventListener("click", () => closeConfirmation(true));
document.querySelector("#action-modal")?.addEventListener("click", (event) => { if (event.target === event.currentTarget) closeConfirmation(false); });
libraryModal.addEventListener("click", (event) => { if (event.target === libraryModal) libraryModal.classList.add("hidden"); });
projectsModal.addEventListener("click", (event) => { if (event.target === projectsModal) projectsModal.classList.add("hidden"); });
guideModal.addEventListener("click", (event) => { if (event.target === guideModal) guideModal.classList.add("hidden"); });
readerClose.addEventListener("click", () => readerModal.classList.add("hidden"));
readerModal.addEventListener("click", (event) => { if (event.target === readerModal) readerModal.classList.add("hidden"); });
libraryFilter.addEventListener("input", renderLibrary);
document.querySelector("#import-button")?.addEventListener("click", () => { void importDocument(); });
document.querySelector("#rebuild-button")?.addEventListener("click", async () => { await call("rebuild_user_index"); showToast("个人资料全文索引已重建。", "success"); });
document.querySelector("#create-project-button")?.addEventListener("click", () => { void createProject(); });
projectSelect.addEventListener("change", () => { localStorage.setItem("ilia.project", projectSelect.value); const activeProject = projects.find((project) => project.id === projectSelect.value); document.querySelector<HTMLElement>("#workspace-context")!.textContent = activeProject?.title ?? "新研究"; syncProjectWorkspaceContext(); void loadProjectNote(); });
document.querySelector("#save-answer-button")?.addEventListener("click", () => { void saveResearchToProject(); });
document.querySelector("#export-md")?.addEventListener("click", () => { void exportCurrent("markdown"); });
document.querySelector("#export-html")?.addEventListener("click", () => { void exportCurrent("html"); });
document.querySelector<HTMLTextAreaElement>("#project-note")?.addEventListener("input", (event) => {
  if (noteTimer !== null) window.clearTimeout(noteTimer);
  const projectId = projectSelect.value;
  const body = (event.target as HTMLTextAreaElement).value;
  if (!projectId) return;
  projectNoteDrafts.set(projectId, body);
  document.querySelector<HTMLElement>("#project-status")!.textContent = "正在保存笔记…";
  noteTimer = window.setTimeout(() => {
    noteTimer = null;
    noteSaveChain = noteSaveChain.then(async () => {
      try {
        const saved = await call<{ id: string }>("save_note", { projectId, noteId: projectNoteIds.get(projectId) ?? null, body });
        projectNoteIds.set(projectId, saved.id);
        if (projectNoteDrafts.get(projectId) === body) projectNoteDrafts.delete(projectId);
        if (projectSelect.value === projectId && !projectNoteDrafts.has(projectId)) {
          document.querySelector<HTMLElement>("#project-status")!.textContent = "笔记已自动保存";
        }
      } catch (error) {
        if (projectSelect.value === projectId) document.querySelector<HTMLElement>("#project-status")!.textContent = `保存失败：${errorMessage(error)}；编辑内容仍保留。`;
      }
    });
  }, 600);
});
question.addEventListener("keydown", (event) => { if ((event.ctrlKey || event.metaKey) && event.key === "Enter") runAsk(); });
researchMode.addEventListener("change", updateModeHelper);
document.querySelectorAll<HTMLButtonElement>(".example").forEach((button) => button.addEventListener("click", () => { question.value = button.dataset.question ?? button.querySelector("strong")?.textContent ?? button.textContent ?? ""; document.querySelectorAll(".history-item").forEach((item) => item.classList.toggle("active", item === button)); question.focus(); }));
document.querySelector("#first-run-guide")?.addEventListener("click", () => openGuide());
document.querySelector("#first-run-dismiss")?.addEventListener("click", () => { localStorage.setItem("ilia.guide-dismissed", "true"); document.querySelector("#first-run-tip")?.classList.add("hidden"); });
backendSelect.addEventListener("change", async () => { try { updateRuntime(await call<RuntimeProbeReport>("set_runtime_preference", { preference: backendSelect.value })); } catch (error) { showError(error); } });
sourceLink.addEventListener("click", async (event) => {
  if (!isTauri()) return;
  event.preventDefault();
  try { await openUrl(sourceLink.href); } catch (error) { showError(error); }
});
translateButton.addEventListener("click", () => { void translateCurrentSource(); });
parallelButton.addEventListener("click", () => { preferredSourceLanguage = "parallel"; void translateCurrentSource(false); });
originalButton.addEventListener("click", () => {
  const evidence = currentEvidence[currentSourceIndex];
  const hit = evidence ? currentHits.find((item) => item.stable_key === evidence.stable_key) : undefined;
  if (hit) {
    preferredSourceLanguage = "original";
    document.querySelector<HTMLElement>("#source-language")!.textContent = hit.language.toUpperCase();
    showOriginalSource(hit);
  }
});
document.querySelector<HTMLSelectElement>("#performance-preset")?.addEventListener("change", async (event) => { const preset = (event.target as HTMLSelectElement).value as PerformancePreset; await call("set_performance_preset", { preset }); document.querySelector<HTMLElement>("#resource-report")!.textContent = "性能档位已更新；现有模型会话已释放，下次预热或问答时生效。"; });
document.querySelector("#prewarm-button")?.addEventListener("click", async (event) => { const button = event.currentTarget as HTMLButtonElement; button.disabled = true; button.textContent = "预热中…"; try { const report = await call<RuntimeStartupReport>("prewarm_model"); updateRuntime(report.detection, report); document.querySelector<HTMLElement>("#resource-report")!.textContent = `${report.selected_backend.toUpperCase()} · 上下文 ${report.selected_profile.context_size.toLocaleString()} · GPU 层 ${report.selected_profile.gpu_layers} · 估算内存 ${report.selected_profile.estimated_memory_mib.toLocaleString()} MiB`; } catch (error) { document.querySelector<HTMLElement>("#resource-report")!.textContent = `预热失败（不影响纯检索）：${errorMessage(error)}`; } finally { button.disabled = false; button.textContent = "后台预热模型"; } });
document.querySelector("#save-mirror")?.addEventListener("click", () => {
  const input = document.querySelector<HTMLInputElement>("#mirror-url")!;
  const value = input.value.trim().replace(/\/+$/, "");
  if (value) {
    try {
      const parsed = new URL(value);
      if (parsed.protocol !== "https:") throw new Error("镜像必须使用 HTTPS");
    } catch (error) {
      showToast(`镜像地址无效：${errorMessage(error)}`, "error");
      return;
    }
    localStorage.setItem("ilia.update-mirror", value);
  } else {
    localStorage.removeItem("ilia.update-mirror");
  }
  document.querySelector<HTMLElement>("#mirror-status")!.textContent = value ? `优先使用：${value}；失败时回退 GitHub。签名与 SHA-256 校验不会跳过。` : "未配置正式镜像；将使用 GitHub 官方发布地址。";
  showToast(value ? "镜像配置已保存。" : "镜像配置已清除。", "success");
});
document.querySelector("#save-proxy")?.addEventListener("click", async () => {
  const input = document.querySelector<HTMLInputElement>("#proxy-url")!;
  try {
    const settings = await call<ProxySettings>("set_proxy_settings", { url: input.value.trim() || null });
    input.value = "";
    const source = { manual: "手动设置", environment: "系统环境", system: "Windows 系统代理" }[settings.source ?? "manual"];
    document.querySelector<HTMLElement>("#proxy-status")!.textContent = settings.enabled ? `已启用（${source}）：${settings.redacted_url}` : "未检测到代理；在线更新将直接连接。";
  } catch (error) { showToast(`代理设置无效：${errorMessage(error)}`, "error"); }
});
document.querySelector("#local-update-button")?.addEventListener("click", async () => {
  const path = document.querySelector<HTMLInputElement>("#local-update-path")!.value.trim();
  if (!path) return;
  const button = document.querySelector<HTMLButtonElement>("#local-update-button")!;
  button.disabled = true;
  button.textContent = "正在验证…";
  try {
    const status = await call<UpdateStatus>("inspect_local_update", { packagePath: path });
    const available = status.manifest.components.filter((component) => status.installed_versions.components[component.id] !== component.version);
    const totalBytes = available.reduce((sum, component) => sum + component.payload_size, 0);
    const details = available.length ? available.map((component) => `• ${updateComponentLabel(component)}`).join("\n") : "• 所有组件已经是当前版本";
    const confirmed = await confirmAction({ title: `安装 ${status.manifest.release_id}`, message: `当前版本：${currentApplicationVersion}\n目标版本：${status.manifest.release_id.replace(/^v/, "")}\n更新来源：本地签名包\n更新体积：${formatBytes(totalBytes)}\n\n${details}\n\n签名与 SHA-256 已通过验证；安装失败不会替换当前版本。`, confirmLabel: "安装更新", kicker: "本地安全更新" });
    if (!confirmed || !available.length) return;
    lastUpdateReleaseId = status.manifest.release_id;
    lastLocalUpdatePath = path;
    updateDownloadSample = null;
    showUpdateProgress({ phase: "checking", release_id: status.manifest.release_id, component_id: null, downloaded_bytes: 0, total_bytes: 0, message_zh: "正在准备已验证的本地更新包" });
    await call("install_local_update", { packagePath: path });
  } catch (error) { showToast(`本地更新未启动：${errorMessage(error)}`, "error"); }
  finally { button.disabled = false; button.textContent = "检查并安装"; }
});
document.querySelector("#local-update-choose")?.addEventListener("click", () => { void chooseLocalUpdatePackage(); });
document.querySelector("#local-update-drop")?.addEventListener("click", () => { void chooseLocalUpdatePackage(); });
document.querySelector("#local-update-drop")?.addEventListener("keydown", (event) => {
  if (event instanceof KeyboardEvent && (event.key === "Enter" || event.key === " ")) { event.preventDefault(); void chooseLocalUpdatePackage(); }
});
document.querySelector("#backup-button")?.addEventListener("click", async () => {
  const input = document.querySelector<HTMLInputElement>("#backup-path")!; const status = document.querySelector<HTMLElement>("#backup-status")!; const outputPath = input.value.trim();
  if (!outputPath) return input.focus(); status.textContent = "正在检查数据库并创建备份…";
  try { const result = await call<BackupSummary>("create_backup", { outputPath }); status.textContent = `备份完成：${result.path}（${((result.user_bytes + result.workspace_bytes) / 1024).toFixed(1)} KiB）`; }
  catch (error) { status.textContent = `备份失败，原数据未改变：${errorMessage(error)}`; }
});
document.querySelector("#restore-button")?.addEventListener("click", async () => {
  const input = document.querySelector<HTMLInputElement>("#restore-path")!; const status = document.querySelector<HTMLElement>("#backup-status")!; const packagePath = input.value.trim();
  if (!packagePath) return input.focus();
  const confirmed = await confirmAction({ title: "恢复研究工作区", message: `将使用以下备份替换当前个人资料库与研究工作区：\n${packagePath}\n\n恢复前请确认已经为当前数据创建备份。`, confirmLabel: "校验并恢复", kicker: "研究数据保障", danger: true });
  if (!confirmed) return;
  status.textContent = "正在校验备份并安全恢复…";
  try { const result = await call<BackupSummary>("restore_backup", { packagePath }); await loadProjects(); status.textContent = `恢复完成：${result.path}。项目与个人资料已重新载入。`; }
  catch (error) { status.textContent = `恢复失败，当前数据未被替换：${errorMessage(error)}`; }
});
document.querySelector("#copy-citation")?.addEventListener("click", () => { void copyCurrentCitation(); });
document.querySelector<HTMLSelectElement>("#citation-style")?.addEventListener("change", () => { void updateCitationPreview(); });

if (isTauri()) {
  void listen<ResearchEventEnvelope>("research-event", (event) => handleResearchEvent(event.payload));
  void listen<UpdateProgress>("update-progress", (event) => showUpdateProgress(event.payload));
  void getCurrentWebview().onDragDropEvent((event) => {
    const drop = document.querySelector<HTMLElement>("#local-update-drop")!;
    if (settingsModal.classList.contains("hidden")) return;
    if (event.payload.type === "enter" || event.payload.type === "over") drop.classList.add("dragging");
    if (event.payload.type === "leave") drop.classList.remove("dragging");
    if (event.payload.type === "drop") {
      drop.classList.remove("dragging");
      const packagePath = event.payload.paths.find((path) => path.toLowerCase().endsWith(".ilia"));
      if (packagePath) setLocalUpdatePackage(packagePath);
      else showToast("拖入的文件不是 .ilia 签名更新包。", "error");
    }
  });
}
window.addEventListener("beforeunload", () => {
  if (activeRequestId && isTauri()) void invoke("cancel_research", { requestId: activeRequestId });
});
document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  if (!document.querySelector("#action-modal")?.classList.contains("hidden")) {
    closeConfirmation(false);
    return;
  }
  const visible = Array.from(document.querySelectorAll<HTMLElement>(".library-modal:not(.hidden)"));
  const top = visible.at(-1);
  if (top && top.id !== "update-progress-modal") top.classList.add("hidden");
});

call<RuntimeProbeReport>("get_runtime_status").then(updateRuntime).catch(showError);
updateModeHelper();
if (!localStorage.getItem("ilia.guide-completed") && !localStorage.getItem("ilia.guide-dismissed")) document.querySelector("#first-run-tip")?.classList.remove("hidden");
void loadProjects().catch(() => { document.querySelector<HTMLElement>("#workspace-context")!.textContent = "新研究"; });
void refreshUpdateSummary(true).catch(() => { /* 更新状态不影响离线研究工作流。 */ });
window.setTimeout(() => { void checkForUpdates(true); }, 2500);
const idleTimeout = document.querySelector<HTMLSelectElement>("#idle-timeout")!;
idleTimeout.value = localStorage.getItem("ilia.idle-timeout") ?? "900";
idleTimeout.addEventListener("change", () => localStorage.setItem("ilia.idle-timeout", idleTimeout.value));
window.setInterval(() => { const seconds = Number(idleTimeout.value); if (seconds > 0 && isTauri()) void invoke<boolean>("release_idle_model", { idleSeconds: seconds }).then((released) => { if (released) document.querySelector<HTMLElement>("#resource-report")!.textContent = "模型已因空闲释放显存；下一次问答会自动恢复。"; }); }, 60_000);
