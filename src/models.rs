use anyhow::{Context, Result};
use model2vec_rs::model::StaticModel;
use std::collections::HashMap;

use crate::config::ModelConfig;

pub struct ModelRegistry {
    models: HashMap<String, StaticModel>,
}

impl ModelRegistry {
    pub fn load_with_token(configs: &[ModelConfig], hf_token: Option<&str>) -> Result<Self> {
        let mut models = HashMap::with_capacity(configs.len());

        for cfg in configs {
            log::info!("loading model alias={} path={}", cfg.alias, cfg.path);

            let model = StaticModel::from_pretrained(&cfg.path, hf_token, None, None)
                .with_context(|| {
                    format!("failed to load model '{}' from '{}'", cfg.alias, cfg.path)
                })?;

            log::info!("model loaded alias={}", cfg.alias);
            models.insert(cfg.alias.clone(), model);
        }

        Ok(Self { models })
    }

    pub fn encode(&self, alias: &str, texts: &[String]) -> Option<Vec<Vec<f32>>> {
        let model = self.models.get(alias)?;
        Some(model.encode(texts))
    }

    pub fn aliases(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.models.keys().map(|s| s.as_str()).collect();
        names.sort_unstable();
        names
    }
}
