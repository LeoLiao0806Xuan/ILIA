use ilia_core::{CitationSupport, EvidenceItem, LibraryKind, StableEvidenceKey};
use ilia_inference::{
    AnswerService, CancellationToken, PerformancePreset, RuntimeManager, RuntimeManagerConfig,
    RuntimePreference,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
struct Case {
    id: String,
    statement: String,
    evidence: String,
    expected_support: CitationSupport,
    language: String,
}
#[derive(Serialize)]
struct CaseResult {
    id: String,
    language: String,
    expected: CitationSupport,
    predicted: CitationSupport,
    passed: bool,
}
#[derive(Default, Serialize)]
struct Metrics {
    total: usize,
    correct: usize,
    precision: f64,
    recall: f64,
}
#[derive(Serialize)]
struct Report {
    report_version: u32,
    release_version: String,
    generated_at_unix: u64,
    model_backend: String,
    total: usize,
    correct: usize,
    accuracy: f64,
    by_label: BTreeMap<String, Metrics>,
    cases: Vec<CaseResult>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mut root = None;
    let mut input = None;
    let mut output = None;
    let mut version = "1.1.0-dev".to_owned();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = args.next().map(PathBuf::from),
            "--input" => input = args.next().map(PathBuf::from),
            "--output" => output = args.next().map(PathBuf::from),
            "--version" => version = args.next().ok_or("missing version")?,
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    let root = root.ok_or("missing --root")?;
    let input = input.ok_or("missing --input")?;
    let output = output.ok_or("missing --output")?;
    let cases = fs::read_to_string(input)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<Vec<Case>, _>>()?;
    let api_key = uuid::Uuid::new_v4().simple().to_string();
    let server = RuntimeManager::start(&RuntimeManagerConfig {
        runtime_root: root.join("runtime"),
        model: root.join("models/qwen3-4b/Qwen3-4B-Q4_K_M.gguf"),
        preference: RuntimePreference::Auto,
        performance_preset: PerformancePreset::Balanced,
        host: "127.0.0.1".into(),
        port: available_port()?,
        startup_timeout: Duration::from_secs(120),
        log_dir: root.join("target/citation-eval-logs"),
        api_key: Some(api_key.clone()),
    })?;
    let service = AnswerService::new(server.base_url()).with_api_key(api_key);
    let cancellation = CancellationToken::default();
    let mut results = Vec::new();
    for (index, case) in cases.into_iter().enumerate() {
        let evidence = EvidenceItem {
            rank: 1,
            stable_key: StableEvidenceKey::new(
                LibraryKind::Core,
                "citation-eval",
                format!("case-{index}"),
            )?,
            chunk_id: format!("case-{index}"),
            related_chunk_ids: vec![],
            citation_label: "Evaluation evidence".into(),
            selection_reason: "human reference set".into(),
            text: case.evidence,
        };
        let predicted =
            service.classify_citation_support(&case.statement, &evidence, &cancellation)?;
        results.push(CaseResult {
            id: case.id,
            language: case.language,
            expected: case.expected_support,
            predicted,
            passed: predicted == case.expected_support,
        });
    }
    let mut by_label = BTreeMap::new();
    for label in [
        CitationSupport::Direct,
        CitationSupport::Summary,
        CitationSupport::Unsupported,
        CitationSupport::Conflict,
    ] {
        let name = format!("{label:?}").to_lowercase();
        let tp = results
            .iter()
            .filter(|r| r.expected == label && r.predicted == label)
            .count();
        let predicted = results.iter().filter(|r| r.predicted == label).count();
        let expected = results.iter().filter(|r| r.expected == label).count();
        by_label.insert(
            name,
            Metrics {
                total: expected,
                correct: tp,
                precision: tp as f64 / predicted.max(1) as f64,
                recall: tp as f64 / expected.max(1) as f64,
            },
        );
    }
    let correct = results.iter().filter(|r| r.passed).count();
    let report = Report {
        report_version: 1,
        release_version: version,
        generated_at_unix: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        model_backend: server.backend().as_str().into(),
        total: results.len(),
        correct,
        accuracy: correct as f64 / results.len().max(1) as f64,
        by_label,
        cases: results,
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
fn available_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}
