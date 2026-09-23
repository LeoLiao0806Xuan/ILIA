use std::{env, path::PathBuf};

use fastembed::{
    Bgem3Embedding, Bgem3InitOptions, Bgem3Model, InitOptionsUserDefined, TokenizerFiles,
};
use thiserror::Error;

pub const BGE_M3_MODEL_ID: &str = "gpahal/bge-m3-onnx-int8";
pub const BGE_M3_DIMENSION: usize = 1024;

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("BGE-M3 embedding error: {0}")]
    FastEmbed(#[from] fastembed::Error),
    #[error("BGE-M3 model file error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct BgeM3Embedder {
    model: Bgem3Embedding,
}

impl BgeM3Embedder {
    pub fn new(cache_dir: PathBuf, show_download_progress: bool) -> Result<Self, EmbeddingError> {
        let execution_providers = match env::var("ILIA_EMBEDDING_BACKEND") {
            Ok(value) if value.eq_ignore_ascii_case("directml") => vec![
                ort::ep::DirectML::default()
                    .with_performance_preference(
                        ort::ep::directml::PerformancePreference::HighPerformance,
                    )
                    .build(),
            ],
            _ => Vec::new(),
        };
        let local_model = cache_dir.join("model.onnx");
        if local_model.exists() {
            let tokenizer_files = TokenizerFiles {
                tokenizer_file: std::fs::read(cache_dir.join("tokenizer.json"))?,
                config_file: std::fs::read(cache_dir.join("config.json"))?,
                special_tokens_map_file: std::fs::read(cache_dir.join("special_tokens_map.json"))?,
                tokenizer_config_file: std::fs::read(cache_dir.join("tokenizer_config.json"))?,
            };
            return Ok(Self {
                model: Bgem3Embedding::try_new_from_path(
                    local_model,
                    tokenizer_files,
                    InitOptionsUserDefined::new()
                        .with_max_length(512)
                        .with_execution_providers(execution_providers),
                )?,
            });
        }
        let options = Bgem3InitOptions::new(Bgem3Model::BGEM3Q)
            .with_cache_dir(cache_dir)
            .with_max_length(512)
            .with_execution_providers(execution_providers)
            .with_show_download_progress(show_download_progress);
        Ok(Self {
            model: Bgem3Embedding::try_new(options)?,
        })
    }

    pub fn embed(
        &mut self,
        texts: &[String],
        batch_size: usize,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        Ok(self.model.embed(texts, Some(batch_size))?.dense)
    }

    pub fn embed_query(&mut self, query: &str) -> Result<Vec<f32>, EmbeddingError> {
        let output = self.model.embed([query], Some(1))?;
        Ok(output.dense.into_iter().next().unwrap_or_default())
    }
}
