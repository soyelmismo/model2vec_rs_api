use anyhow::{Context, Result};
use model2vec_rs::model::StaticModel;
use std::collections::HashMap;

use crate::config::ModelConfig;

pub struct ModelRegistry {
    models: HashMap<String, StaticModel>,
    dims: HashMap<String, usize>,
    sorted_aliases: Vec<String>,
}

impl ModelRegistry {
    pub fn load_with_token(configs: &[ModelConfig], hf_token: Option<&str>) -> Result<Self> {
        let mut models = HashMap::with_capacity(configs.len());
        let mut dims = HashMap::with_capacity(configs.len());

        for cfg in configs {
            log::info!("loading model alias={} path={}", cfg.alias, cfg.path);

            let model = StaticModel::from_pretrained(&cfg.path, hf_token, None, None)
                .with_context(|| {
                    format!("failed to load model '{}' from '{}'", cfg.alias, cfg.path)
                })?;

            let dim = model.encode(&["probe".to_owned()]).into_iter().next().map_or(0, |v| v.len());

            log::info!("model loaded successfully alias={} dims={}", cfg.alias, dim);

            if let Some(prev) = models.insert(cfg.alias.clone(), model) {
                log::warn!(
                    "model alias '{}' was loaded twice — replacing previous instance",
                    cfg.alias
                );
                drop(prev);
            }
            let _ = dims.insert(cfg.alias.clone(), dim);
        }

        let mut sorted_aliases: Vec<String> = models.keys().cloned().collect();
        sorted_aliases.sort_unstable();

        Ok(Self {
            models,
            dims,
            sorted_aliases,
        })
    }

    pub fn encode_owned(&self, alias: &str, texts: &[String]) -> Option<Vec<Vec<f32>>> {
        let model = self.models.get(alias)?;
        Some(model.encode(texts))
    }

    pub fn dims(&self, alias: &str) -> Option<usize> {
        self.dims.get(alias).copied()
    }

    pub fn aliases(&self) -> &[String] {
        &self.sorted_aliases
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_registry() -> ModelRegistry {
        ModelRegistry::load_with_token(&[], None).unwrap()
    }

    #[test]
    fn encode_missing_model_returns_none() {
        let reg = empty_registry();
        assert!(reg.encode_owned("nonexistent", &["text".to_owned()]).is_none());
    }

    #[test]
    fn dims_missing_model_returns_none() {
        let reg = empty_registry();
        assert!(reg.dims("nonexistent").is_none());
    }

    #[test]
    fn aliases_empty_when_no_models() {
        let reg = empty_registry();
        assert!(reg.aliases().is_empty());
    }
}
