use anyhow::{Context, Result};
use model2vec_rs::model::StaticModel;
use std::collections::HashMap;
use std::path::PathBuf;
use tokenizers::Tokenizer;

use crate::config::ModelConfig;

pub struct ModelRegistry {
    models: HashMap<String, StaticModel>,
    tokenizers: HashMap<String, Tokenizer>,
}

impl ModelRegistry {
    pub fn load_with_token(configs: &[ModelConfig], hf_token: Option<&str>) -> Result<Self> {
        let mut models = HashMap::with_capacity(configs.len());
        let mut tokenizers = HashMap::with_capacity(configs.len());

        for cfg in configs {
            log::info!("loading model alias={} path={}", cfg.alias, cfg.path);

            let model = StaticModel::from_pretrained(&cfg.path, hf_token, None, None)
                .with_context(|| {
                    format!("failed to load model '{}' from '{}'", cfg.alias, cfg.path)
                })?;

            let tokenizer_path = find_tokenizer_json(&cfg.path)
                .context(format!("tokenizer.json not found for '{}'", cfg.alias))?;

            let tokenizer = Tokenizer::from_file(&tokenizer_path)
                .map_err(|e| anyhow::anyhow!(
                    "failed to load tokenizer for '{}' from {}: {e}",
                    cfg.alias, tokenizer_path.display()
                ))?;

            log::info!("model + tokenizer loaded alias={}", cfg.alias);
            models.insert(cfg.alias.clone(), model);
            tokenizers.insert(cfg.alias.clone(), tokenizer);
        }

        Ok(Self { models, tokenizers })
    }

    pub fn encode(&self, alias: &str, texts: &[String]) -> Option<(Vec<Vec<f32>>, Vec<usize>)> {
        let model = self.models.get(alias)?;
        let tok = self.tokenizers.get(alias)?;

        let embeddings = model.encode(texts);
        let token_counts: Vec<usize> = texts
            .iter()
            .map(|t| tok.encode(t.as_str(), false).map(|e| e.len()).unwrap_or(0))
            .collect();

        Some((embeddings, token_counts))
    }

    pub fn aliases(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.models.keys().map(|s| s.as_str()).collect();
        names.sort_unstable();
        names
    }
}

/// Locate `tokenizer.json` either locally or in HuggingFace's cache.
/// model2vec-rs already downloaded the model files during `from_pretrained`,
/// so the tokenizer should be present in the HF cache if not local.
fn find_tokenizer_json(path: &str) -> Result<PathBuf> {
    let p = std::path::Path::new(path);

    // Direct local path
    let local = p.join("tokenizer.json");
    if local.exists() {
        return Ok(local);
    }

    // HuggingFace cache: ~/.cache/huggingface/hub/models--owner--name/snapshots/<hash>/
    let repo_dir = path.replace('/', "--");
    let home = std::env::var("HOME").unwrap_or_default();
    let cache_base = PathBuf::from(home).join(".cache/huggingface/hub");
    let repo_cache = cache_base.join(format!("models--{repo_dir}"));

    let snapshots = repo_cache.join("snapshots");
    if snapshots.exists() {
        if let Ok(mut entries) = std::fs::read_dir(&snapshots) {
            while let Some(Ok(entry)) = entries.next() {
                let tf = entry.path().join("tokenizer.json");
                if tf.exists() {
                    return Ok(tf);
                }
            }
        }
    }

    anyhow::bail!(
        "tokenizer.json not found for '{path}' — looked locally at {} and in HF cache",
        local.display()
    )
}
