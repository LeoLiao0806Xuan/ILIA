use std::{env, net::TcpListener, path::PathBuf, process::ExitCode, time::Duration};

use ilia_core::SearchOptions;
use ilia_database::Database;
use ilia_embedding::{BGE_M3_MODEL_ID, BgeM3Embedder};
use ilia_inference::{
    AnswerService, RuntimeManager, RuntimeManagerConfig, RuntimePreference, RuntimeStartupReport,
};
use ilia_retrieval::RetrievalService;
use serde::Serialize;

#[derive(Serialize)]
struct AskOutput<'a> {
    runtime: &'a RuntimeStartupReport,
    answer: ilia_core::AnswerResponse,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut database: Option<PathBuf> = None;
    let mut bge_cache: Option<PathBuf> = None;
    let mut runtime_root = PathBuf::from("runtime");
    let mut backend = RuntimePreference::Auto;
    let mut qwen_model: Option<PathBuf> = None;
    let mut question: Option<String> = None;
    let mut log_dir = PathBuf::from("data");
    let mut port: Option<u16> = None;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--db" => database = args.next().map(PathBuf::from),
            "--bge-cache" => bge_cache = args.next().map(PathBuf::from),
            "--runtime-root" => {
                runtime_root = PathBuf::from(args.next().ok_or("missing --runtime-root value")?)
            }
            "--backend" => {
                backend = args
                    .next()
                    .ok_or("missing --backend value")?
                    .parse()
                    .map_err(|error: String| error)?
            }
            "--qwen-model" => qwen_model = args.next().map(PathBuf::from),
            "--question" => question = args.next(),
            "--log-dir" => log_dir = PathBuf::from(args.next().ok_or("missing --log-dir value")?),
            "--port" => port = Some(args.next().ok_or("missing --port value")?.parse()?),
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    let usage = "usage: ilia-ask --db <sqlite> --bge-cache <dir> --runtime-root <dir> --backend <auto|cuda|vulkan|cpu> --qwen-model <gguf> --question <text>";
    let database = database.ok_or(usage)?;
    let bge_cache = bge_cache.ok_or(usage)?;
    let qwen_model = qwen_model.ok_or(usage)?;
    let question = question.ok_or(usage)?;
    let port = port.unwrap_or(available_loopback_port()?);
    let api_key = uuid::Uuid::new_v4().simple().to_string();

    let mut embedder = BgeM3Embedder::new(bge_cache, false)?;
    let query_vector = embedder.embed_query(&question)?;
    let retrieval = RetrievalService::new(Database::open_read_only(database)?);
    let search = retrieval.search_hybrid(
        &question,
        &query_vector,
        BGE_M3_MODEL_ID,
        SearchOptions::default(),
    )?;

    let server = RuntimeManager::start(&RuntimeManagerConfig {
        runtime_root,
        model: qwen_model,
        preference: backend,
        host: "127.0.0.1".to_owned(),
        port,
        startup_timeout: Duration::from_secs(120),
        log_dir,
        api_key: Some(api_key.clone()),
    })?;
    let response = AnswerService::new(server.base_url())
        .with_api_key(api_key)
        .answer(&question, &search.evidence)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&AskOutput {
            runtime: server.report(),
            answer: response,
        })?
    );
    Ok(())
}

fn available_loopback_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}
