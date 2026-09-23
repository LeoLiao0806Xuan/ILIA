import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./styles.css";

type Backend = "auto" | "cuda" | "vulkan" | "cpu";

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
  chunk_id: string;
  citation_label: string;
  selection_reason: string;
  text: string;
}

interface SearchResponse {
  query: string;
  hits: SearchHit[];
  evidence: EvidenceItem[];
}

interface AnswerResponse {
  answer: string;
  grounded: boolean;
  evidence: EvidenceItem[];
  generation_ms: number;
  warnings: string[];
}

interface AskResponse {
  runtime: RuntimeStartupReport;
  search: SearchResponse;
  answer: AnswerResponse;
}

interface UpdateComponent {
  id: string;
  kind: "application" | "corpus" | "model" | "runtime";
  version: string;
}

interface UpdateStatus {
  manifest: { release_id: string; components: UpdateComponent[] };
  installed_versions: { components: Record<string, string> };
}

const updateManifestUrl = "https://github.com/LeoLiao0806Xuan/ILIA/releases/latest/download/update-manifest.json";
const updateSignatureUrl = "https://github.com/LeoLiao0806Xuan/ILIA/releases/latest/download/update-manifest.sig";

const demoHit: SearchHit = {
  chunk_id: "unclos-1982-art-3-en",
  document_id: "unclos-1982",
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

const isTauri = () => "__TAURI_INTERNALS__" in window;

async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (isTauri()) return invoke<T>(command, args);
  await new Promise((resolve) => window.setTimeout(resolve, command === "ask_question" ? 650 : 220));
  if (command === "get_runtime_status" || command === "set_runtime_preference") return demoRuntime as T;
  if (command === "check_updates") return { manifest: { release_id: "demo", components: [] }, installed_versions: { components: {} } } as T;
  if (command === "install_update") return undefined as T;
  const search: SearchResponse = { query: String(args.query ?? ""), hits: [demoHit], evidence: [{ rank: 1, chunk_id: demoHit.chunk_id, citation_label: demoHit.citation_label, selection_reason: "RRF fusion of FTS5 and BGE-M3", text: demoHit.text }] };
  if (command === "search_documents") return search as T;
  return {
    runtime: { detection: demoRuntime, selected_backend: "cuda", selected_profile: { context_size: 16384, device: "CUDA0" }, attempts: [{ backend: "cuda", started: true, error: null }] },
    search,
    answer: { answer: "《联合国海洋法公约》规定，领海宽度不得超过 12 海里【1】。", grounded: true, evidence: search.evidence, generation_ms: 579, warnings: [] },
  } as T;
}

document.querySelector<HTMLDivElement>("#app")!.innerHTML = `
  <div class="shell">
    <header class="topbar">
      <div class="brand-block">
        <div class="brand-mark">IL</div>
        <div><div class="brand">ILIA</div><div class="brand-subtitle">International Law Intelligence Assistant</div></div>
      </div>
      <div class="top-actions">
        <button class="update-button" id="update-button">检查更新</button>
        <div class="runtime-pill" id="runtime-pill"><span class="pulse"></span><span id="runtime-text">正在探测运行环境</span></div>
        <label class="backend-control">运行方式
          <select id="backend-select" aria-label="运行方式">
            <option value="auto">自动选择</option><option value="cuda">NVIDIA CUDA</option><option value="vulkan">通用 Vulkan</option><option value="cpu">仅 CPU</option>
          </select>
        </label>
      </div>
    </header>

    <main class="workspace">
      <aside class="query-column">
        <div class="section-kicker">研究问题</div>
        <h1>从法律原文出发，<br/>得到可核验的回答。</h1>
        <p class="lead">回答仅依据本地资料库。每项结论均可回到对应条款、判例段落与官方来源。</p>
        <label class="question-label" for="question">输入中文或英文问题</label>
        <textarea id="question" rows="7">《联合国海洋法公约》规定领海宽度不得超过多少海里？</textarea>
        <div class="query-actions">
          <button class="primary" id="ask-button"><span>生成有据回答</span><span aria-hidden="true">→</span></button>
          <button class="secondary" id="search-button">只检索资料</button>
        </div>
        <div class="examples">
          <div class="examples-title">示例问题</div>
          <button class="example">国家能否以国内法为理由不履行条约？</button>
          <button class="example">尼加拉瓜案第191段如何说明法律确信？</button>
          <button class="example">《联合国宪章》第51条规定了什么？</button>
        </div>
        <div class="privacy-note"><span>●</span><div><strong>完全本地运行</strong><br/>问题、资料和回答不会发送到外部服务。</div></div>
      </aside>

      <section class="answer-column">
        <div class="answer-header"><div><div class="section-kicker">分析结果</div><h2 id="result-title">准备就绪</h2></div><div class="grounded-badge hidden" id="grounded-badge">✓ 引证已校验</div></div>
        <div class="empty-state" id="empty-state">
          <div class="empty-glyph">§</div><h3>等待研究问题</h3><p>可以生成回答，也可以先查看检索到的法律资料。</p>
        </div>
        <div class="loading-state hidden" id="loading-state"><div class="loader"></div><h3 id="loading-title">正在检索本地资料</h3><p id="loading-copy">正在运行 FTS5 与 BGE-M3 混合检索。</p></div>
        <article class="answer-card hidden" id="answer-card"><div class="answer-copy" id="answer-copy"></div><div class="answer-meta" id="answer-meta"></div></article>
        <div class="evidence-section hidden" id="evidence-section"><div class="section-row"><h3>检索证据</h3><span id="evidence-count"></span></div><div class="evidence-list" id="evidence-list"></div></div>
      </section>

      <aside class="source-column">
        <div class="source-placeholder" id="source-placeholder"><div class="source-icon">¶</div><h3>原文证据</h3><p>选择一条检索结果，在这里核对原文、页码、法律性质与官方来源。</p></div>
        <div class="source-detail hidden" id="source-detail">
          <div class="section-kicker">原文证据</div><div class="source-index" id="source-index">证据 1</div>
          <h2 id="source-title"></h2><div class="source-chips" id="source-chips"></div>
          <div class="citation-box"><div>规范引用</div><strong id="source-citation"></strong></div>
          <div class="original-heading"><span>原文</span><span id="source-language"></span></div>
          <pre id="source-text"></pre>
          <a id="source-link" class="source-link" target="_blank" rel="noreferrer">查看官方来源 ↗</a>
        </div>
      </aside>
    </main>
  </div>`;

const question = document.querySelector<HTMLTextAreaElement>("#question")!;
const askButton = document.querySelector<HTMLButtonElement>("#ask-button")!;
const searchButton = document.querySelector<HTMLButtonElement>("#search-button")!;
const updateButton = document.querySelector<HTMLButtonElement>("#update-button")!;
const backendSelect = document.querySelector<HTMLSelectElement>("#backend-select")!;
const sourceLink = document.querySelector<HTMLAnchorElement>("#source-link")!;
let currentHits: SearchHit[] = [];
let currentEvidence: EvidenceItem[] = [];

function setBusy(busy: boolean, title = "正在检索本地资料", copy = "正在运行 FTS5 与 BGE-M3 混合检索。") {
  askButton.disabled = busy; searchButton.disabled = busy;
  document.querySelector("#empty-state")?.classList.add("hidden");
  document.querySelector("#answer-card")?.classList.add("hidden");
  document.querySelector("#evidence-section")?.classList.add("hidden");
  document.querySelector("#grounded-badge")?.classList.add("hidden");
  document.querySelector("#loading-state")?.classList.toggle("hidden", !busy);
  document.querySelector("#source-detail")?.classList.toggle("muted", busy);
  document.querySelector<HTMLElement>("#loading-title")!.textContent = title;
  document.querySelector<HTMLElement>("#loading-copy")!.textContent = copy;
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

function citationFragment(text: string): DocumentFragment {
  const fragment = document.createDocumentFragment();
  const expression = /【(\d+)】/g;
  let cursor = 0;
  for (const match of text.matchAll(expression)) {
    fragment.append(document.createTextNode(text.slice(cursor, match.index)));
    const button = document.createElement("button");
    button.className = "citation-token"; button.textContent = match[0];
    button.addEventListener("click", () => showSource(Number(match[1]) - 1));
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
  const title = document.querySelector<HTMLElement>("#result-title")!;
  if (answer) {
    title.textContent = "有据回答"; answerCopy.replaceChildren(citationFragment(answer.answer)); answerCard.classList.remove("hidden");
    const meta = document.querySelector<HTMLElement>("#answer-meta")!;
    meta.textContent = `${(answer.generation_ms / 1000).toFixed(1)} 秒 · ${runtime?.selected_backend.toUpperCase() ?? "LOCAL"} · ${answer.evidence.length} 条证据`;
    document.querySelector("#grounded-badge")?.classList.toggle("hidden", !answer.grounded);
  } else {
    title.textContent = "检索结果"; answerCard.classList.add("hidden");
  }
  const list = document.querySelector<HTMLElement>("#evidence-list")!; list.replaceChildren();
  response.evidence.forEach((evidence, index) => {
    const hit = response.hits.find((item) => item.chunk_id === evidence.chunk_id);
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
  if (response.evidence.length) showSource(0);
  askButton.disabled = false; searchButton.disabled = false;
}

function showSource(index: number) {
  const cards = Array.from(document.querySelectorAll(".evidence-card")); cards.forEach((card, cardIndex) => card.classList.toggle("active", cardIndex === index));
  const evidence = currentEvidence[index];
  const hit = evidence ? currentHits.find((item) => item.chunk_id === evidence.chunk_id) : undefined;
  if (!hit) return;
  document.querySelector("#source-placeholder")?.classList.add("hidden"); document.querySelector("#source-detail")?.classList.remove("hidden");
  document.querySelector<HTMLElement>("#source-index")!.textContent = `证据 ${index + 1}`;
  document.querySelector<HTMLElement>("#source-title")!.textContent = hit.title_zh ?? hit.canonical_title;
  const chips = document.querySelector<HTMLElement>("#source-chips")!; chips.replaceChildren();
  [hit.document_type, hit.legal_status, `第 ${hit.page_start} 页`].forEach((value) => { const chip = document.createElement("span"); chip.textContent = value; chips.append(chip); });
  document.querySelector<HTMLElement>("#source-citation")!.textContent = hit.citation_label;
  document.querySelector<HTMLElement>("#source-language")!.textContent = hit.language.toUpperCase();
  document.querySelector<HTMLElement>("#source-text")!.textContent = hit.text;
  const link = document.querySelector<HTMLAnchorElement>("#source-link")!; link.href = hit.official_source_url;
}

async function runSearch() {
  if (!question.value.trim()) return question.focus();
  setBusy(true);
  try { renderSearch(await call<SearchResponse>("search_documents", { query: question.value.trim() })); }
  catch (error) { showError(error); }
}

async function runAsk() {
  if (!question.value.trim()) return question.focus();
  setBusy(true, "正在生成本地回答", "完成证据检索后，将由 Qwen3-4B 依据原文组织回答并校验引证。");
  try { const response = await call<AskResponse>("ask_question", { query: question.value.trim() }); updateRuntime(response.runtime.detection, response.runtime); renderSearch(response.search, response.answer, response.runtime); }
  catch (error) { showError(error); }
}

async function checkForUpdates() {
  const original = updateButton.textContent;
  updateButton.disabled = true;
  updateButton.textContent = "检查中…";
  try {
    const status = await call<UpdateStatus>("check_updates", {
      manifestUrl: updateManifestUrl,
      signatureUrl: updateSignatureUrl,
    });
    const available = status.manifest.components.filter(
      (component) => status.installed_versions.components[component.id] !== component.version,
    );
    if (!available.length) {
      window.alert("当前已经是最新版本。");
      return;
    }
    const summary = available.map((component) => `${component.kind} · ${component.version}`).join("\n");
    if (!window.confirm(`发现 ${status.manifest.release_id} 更新：\n\n${summary}\n\n现在安装并重启 ILIA？`)) return;
    updateButton.textContent = "准备更新…";
    await call<void>("install_update", {
      manifestUrl: updateManifestUrl,
      signatureUrl: updateSignatureUrl,
    });
  } catch (error) {
    window.alert(`更新检查失败：${String(error)}`);
  } finally {
    updateButton.disabled = false;
    updateButton.textContent = original;
  }
}

function showError(error: unknown) {
  setBusy(false); document.querySelector<HTMLElement>("#result-title")!.textContent = "暂时无法完成";
  const empty = document.querySelector<HTMLElement>("#empty-state")!; empty.classList.remove("hidden");
  empty.querySelector("h3")!.textContent = "本地服务发生错误"; empty.querySelector("p")!.textContent = String(error);
  askButton.disabled = false; searchButton.disabled = false;
}

askButton.addEventListener("click", runAsk); searchButton.addEventListener("click", runSearch);
updateButton.addEventListener("click", checkForUpdates);
question.addEventListener("keydown", (event) => { if ((event.ctrlKey || event.metaKey) && event.key === "Enter") runAsk(); });
document.querySelectorAll<HTMLButtonElement>(".example").forEach((button) => button.addEventListener("click", () => { question.value = button.textContent ?? ""; question.focus(); }));
backendSelect.addEventListener("change", async () => { try { updateRuntime(await call<RuntimeProbeReport>("set_runtime_preference", { preference: backendSelect.value })); } catch (error) { showError(error); } });
sourceLink.addEventListener("click", async (event) => {
  if (!isTauri()) return;
  event.preventDefault();
  try { await openUrl(sourceLink.href); } catch (error) { showError(error); }
});

call<RuntimeProbeReport>("get_runtime_status").then(updateRuntime).catch(showError);
