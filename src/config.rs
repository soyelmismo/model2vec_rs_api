use anyhow::{Context, Result};
use std::env;

/// Configuration loaded entirely from environment variables.
///
/// Optional env vars:
///   MODELS       — comma-separated list of `<alias>:<hf_repo_or_local_path>` entries.
///                  Default: base:minishlab/potion-base-8M
///                  Example: MODELS=base:minishlab/potion-base-8M,code:minishlab/potion-code-16M
///   LISTEN_ADDR  — host:port to bind (default: 0.0.0.0:22671)
///   API_KEY      — bearer token required on all requests (disabled if unset)
///   HF_TOKEN     — Hugging Face API token for private/gated models
///   RUST_LOG     — tracing filter string (default: info)
#[derive(Debug, Clone)]
pub struct Config {
    pub listen_addr: String,
    pub models: Vec<ModelConfig>,
    pub api_key: Option<String>,
    /// Reserved: passed to the model loader for private HuggingFace models.
    pub hf_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ModelConfig {
    /// The alias used as the `model` field in OpenAI API requests, e.g. "base"
    pub alias: String,
    /// HuggingFace repo ID (e.g. "minishlab/potion-base-8M") or absolute local path
    pub path: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let listen_addr =
            env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:22671".to_string());

        let models_raw = env::var("MODELS")
            .unwrap_or_else(|_| "base:minishlab/potion-base-8M".to_string());

        let models = parse_models(&models_raw)?;

        // models can only be empty if MODELS was explicitly set to an empty string
        if models.is_empty() {
            anyhow::bail!("MODELS must contain at least one entry");
        }

        let api_key = env::var("API_KEY").ok().filter(|s| !s.is_empty());
        let hf_token = env::var("HF_TOKEN").ok().filter(|s| !s.is_empty());

        Ok(Self {
            listen_addr,
            models,
            api_key,
            hf_token,
        })
    }
}

/// Parse `alias:path,alias2:path2,...` into a list of `ModelConfig`.
fn parse_models(raw: &str) -> Result<Vec<ModelConfig>> {
    raw.split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|entry| {
            let entry = entry.trim();
            let (alias, path) = entry
                .split_once(':')
                .with_context(|| {
                    format!("invalid MODELS entry '{entry}' — expected <alias>:<path>")
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
            parse_models("base:minishlab/potion-base-8M,code:minishlab/potion-code-16M")
                .unwrap();
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
