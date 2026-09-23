use std::{env, path::PathBuf, process::ExitCode};

use ilia_database::Database;
use ilia_embedding::{BGE_M3_DIMENSION, BGE_M3_MODEL_ID, BgeM3Embedder};

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
    let mut cache_dir = PathBuf::from("models/bge-m3");
    let mut batch_size = 2usize;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--db" => database_path = args.next().map(PathBuf::from),
            "--cache-dir" => {
                cache_dir = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or("missing --cache-dir")?
            }
            "--batch-size" => batch_size = args.next().ok_or("missing --batch-size")?.parse()?,
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    if batch_size == 0 {
        return Err("batch size must be greater than zero".into());
    }
    let database_path = database_path
        .ok_or("usage: ilia-corpus-embedder --db <path> [--cache-dir <path>] [--batch-size 2]")?;
    let mut database = Database::open_for_indexing(database_path)?;
    let chunks = database.chunks_for_embedding()?;
    let total = chunks.len();
    let mut embedder = BgeM3Embedder::new(cache_dir, true)?;
    let mut indexed = Vec::with_capacity(total);
    for (batch_number, batch) in chunks.chunks(batch_size).enumerate() {
        let texts = batch
            .iter()
            .map(|chunk| chunk.text.clone())
            .collect::<Vec<_>>();
        let vectors = embedder.embed(&texts, batch_size)?;
        indexed.extend(
            batch
                .iter()
                .zip(vectors)
                .map(|(chunk, vector)| (chunk.chunk_id.clone(), vector)),
        );
        eprintln!(
            "embedded {}/{} chunks (batch {})",
            indexed.len(),
            total,
            batch_number + 1
        );
    }
    database.replace_embeddings(BGE_M3_MODEL_ID, BGE_M3_DIMENSION, &indexed)?;
    println!(
        "stored {} BGE-M3 embeddings in SQLite",
        database.embedding_count(BGE_M3_MODEL_ID)?
    );
    Ok(())
}
