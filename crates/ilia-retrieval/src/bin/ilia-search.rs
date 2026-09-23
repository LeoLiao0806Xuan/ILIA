use std::{env, path::PathBuf, process::ExitCode};

use ilia_core::SearchOptions;
use ilia_database::Database;
use ilia_embedding::{BGE_M3_MODEL_ID, BgeM3Embedder};
use ilia_retrieval::RetrievalService;

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
    let mut args = env::args().skip(1);
    let mut database_path: Option<PathBuf> = None;
    let mut query: Option<String> = None;
    let mut limit = 10usize;
    let mut model_cache: Option<PathBuf> = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--db" => database_path = args.next().map(PathBuf::from),
            "--query" => query = args.next(),
            "--limit" => limit = args.next().ok_or("missing --limit value")?.parse()?,
            "--model-cache" => model_cache = args.next().map(PathBuf::from),
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    let database_path =
        database_path.ok_or("usage: ilia-search --db <path> --query <text> [--limit 10]")?;
    let query = query.ok_or("missing --query")?;
    let service = RetrievalService::new(Database::open_read_only(database_path)?);
    let options = SearchOptions {
        limit,
        ..SearchOptions::default()
    };
    let response = if let Some(cache) = model_cache {
        let mut embedder = BgeM3Embedder::new(cache, false)?;
        let vector = embedder.embed_query(&query)?;
        service.search_hybrid(&query, &vector, BGE_M3_MODEL_ID, options)?
    } else {
        service.search(&query, options)?
    };
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}
