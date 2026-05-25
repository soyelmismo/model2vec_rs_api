use anyhow::{Context, Result};
use model2vec_rs::model::StaticModel;
use std::collections::HashMap;

use crate::config::ModelConfig;

/// Holds all loaded models keyed by their alias.
pub struct ModelRegistry {
    models: HashMap<String, StaticModel>,
}

impl ModelRegistry {
    /// Load every model declared in `configs`, optionally authenticating with `hf_token`.
    #[allow(dead_code)]
    pub fn load(configs: &[ModelConfig]) -> Result<Self> {
        Self::load_with_token(configs, None)
    }

    /// Load every model, forwarding the HuggingFace token for private/gated models.
    pub fn load_with_token(configs: &[ModelConfig], hf_token: Option<&str>) -> Result<Self> {
        let mut models = HashMap::with_capacity(configs.len());

        for cfg in configs {
            log::info!("loading model alias={} path={}", cfg.alias, cfg.path);

            let model = StaticModel::from_pretrained(
                &cfg.path,
                hf_token,   // optional HuggingFace API token
                None,       // normalize — use model's own config
                None,       // subfolder
            )
            .with_context(|| {
                format!(
                    "failed to load model '{}' from '{}'",
                    cfg.alias, cfg.path
                )
            })?;

            log::info!("model loaded alias={}", cfg.alias);
            models.insert(cfg.alias.clone(), model);
        }

        Ok(Self { models })
    }

    /// Encode a batch of texts with the model identified by `alias`.
    /// Returns `None` if the alias is unknown.
    pub fn encode(&self, alias: &str, texts: &[String]) -> Option<Vec<Vec<f32>>> {
        let model = self.models.get(alias)?;
        Some(model.encode(texts))
    }

    /// Returns all known model aliases, sorted alphabetically.
    pub fn aliases(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.models.keys().map(|s| s.as_str()).collect();
        names.sort_unstable();
        names
    }
}
