use anyhow::{Context, Result};
use std::collections::HashMap;
use std::env;

/// Configuration loaded entirely from environment variables.
/// All vars are prefixed with `M2V_` to avoid collisions on shared hosts.
///
/// Optional env vars:
///   `M2V_MODELS`       — comma-separated `<alias>:<hf_repo_or_local_path>` entries.
///                      Default: base:minishlab/potion-base-8M
///   M2V_LISTEN_ADDR  — host:port to bind (default: 0.0.0.0:22671)
///   M2V_API_KEY      — bearer token (disabled if unset)
///   M2V_HF_TOKEN     — Hugging Face token for private models
///   M2V_LOG_LEVEL    — log level: error | warn | info | debug | trace (default: info)
#[derive(Debug, Clone)]
pub struct Config {
    pub listen_addr: String,
    pub models: Vec<ModelConfig>,
    pub api_key: Option<String>,
    pub hf_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub alias: String,
    pub path: String,
}

impl Config {
    /// Build config from env vars, with optional `.env` values as fallback.
    pub fn from_env(dotenv: &HashMap<String, String>) -> Result<Self> {
        let listen_addr = env_val_or("M2V_LISTEN_ADDR", dotenv, "0.0.0.0:22671");

        let models_raw = env_val_or("M2V_MODELS", dotenv, "base:minishlab/potion-base-8M");
        let models = parse_models(&models_raw)?;

        if models.is_empty() {
            anyhow::bail!("M2V_MODELS must contain at least one entry");
        }

        let api_key = env_val_opt("M2V_API_KEY", dotenv);
        let hf_token = env_val_opt("M2V_HF_TOKEN", dotenv);

        Ok(Self {
            listen_addr,
            models,
            api_key,
            hf_token,
        })
    }
}

/// Read env var; fall back to dotenv map, then to default.
fn env_val_or(key: &str, dotenv: &HashMap<String, String>, default: &str) -> String {
    env::var(key)
        .ok()
        .or_else(|| dotenv.get(key).cloned())
        .unwrap_or_else(|| default.to_string())
}

/// Read optional env var; fall back to dotenv map.
fn env_val_opt(key: &str, dotenv: &HashMap<String, String>) -> Option<String> {
    env::var(key)
        .ok()
        .or_else(|| dotenv.get(key).cloned())
        .filter(|s| !s.is_empty())
}

fn parse_models(raw: &str) -> Result<Vec<ModelConfig>> {
    raw.split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|entry| {
            let entry = entry.trim();
            let (alias, path) = entry.split_once(':').with_context(|| {
                format!("invalid M2V_MODELS entry '{entry}' — expected <alias>:<path>")
            })?;
            Ok(ModelConfig {
                alias: alias.trim().to_string(),
                path: path.trim().to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_model() {
        let models = parse_models("base:minishlab/potion-base-8M").unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].alias, "base");
        assert_eq!(models[0].path, "minishlab/potion-base-8M");
    }

    #[test]
    fn parse_multiple_models() {
        let models =
            parse_models("base:minishlab/potion-base-8M,code:minishlab/potion-code-16M").unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[1].alias, "code");
    }

    #[test]
    fn parse_local_path() {
        let models = parse_models("local:/models/my-model").unwrap();
        assert_eq!(models[0].path, "/models/my-model");
    }

    #[test]
    fn missing_colon_fails() {
        assert!(parse_models("nocolon").is_err());
    }

    #[test]
    fn empty_string_is_empty_list() {
        let models = parse_models("").unwrap();
        assert!(models.is_empty());
    }
}
