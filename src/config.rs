use anyhow::{Context, Result};
use std::collections::HashMap;
use std::env;

const ALLOWED_LOCAL_PREFIXES: &[&str] = &["/models/", "/opt/models/", "/data/models/"];

#[derive(Debug, Clone)]
pub struct Config {
    pub listen_addr: String,
    pub models: Vec<ModelConfig>,
    pub api_key: Option<String>,
    pub auth_disabled: bool,
    pub hf_token: Option<String>,
    pub worker_threads: usize,
    pub max_batch_size: usize,
}

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub alias: String,
    pub path: String,
}

impl Config {
    pub fn from_env(dotenv: &HashMap<String, String>) -> Result<Self> {
        let listen_addr = env_val_or("M2V_LISTEN_ADDR", dotenv, "0.0.0.0:22671");

        let models_raw = env_val_or("M2V_MODELS", dotenv, "base:minishlab/potion-base-8M");
        let models = parse_models(&models_raw)?;

        if models.is_empty() {
            anyhow::bail!("M2V_MODELS must contain at least one entry");
        }

        let api_key = env_val_opt("M2V_API_KEY", dotenv);
        let auth_disabled = env_val_opt("M2V_AUTH_DISABLED", dotenv).as_deref() == Some("true");
        let hf_token = env_val_opt("M2V_HF_TOKEN", dotenv);
        let worker_threads = env_val_usize("M2V_WORKER_THREADS", dotenv, 4);
        let max_batch_size = env_val_usize("M2V_MAX_BATCH_SIZE", dotenv, 128);

        if api_key.is_none() && !auth_disabled {
            log::error!(
                "AUTHENTICATION IS DISABLED — no M2V_API_KEY set. \
                 Set M2V_API_KEY=<secret> or explicitly set M2V_AUTH_DISABLED=true \
                 to acknowledge this risk. Exiting for safety."
            );
            anyhow::bail!(
                "M2V_API_KEY is not set. Set M2V_API_KEY=<secret> to enable authentication, \
                 or set M2V_AUTH_DISABLED=true to explicitly disable it (not recommended for production)."
            );
        }

        if auth_disabled {
            log::warn!(
                "AUTHENTICATION IS EXPLICITLY DISABLED (M2V_AUTH_DISABLED=true). \
                 The API is publicly accessible — do NOT expose to untrusted networks."
            );
        }

        Ok(Self {
            listen_addr,
            models,
            api_key,
            auth_disabled,
            hf_token,
            worker_threads,
            max_batch_size,
        })
    }
}

fn env_val_or(key: &str, dotenv: &HashMap<String, String>, default: &str) -> String {
    env::var(key)
        .ok()
        .or_else(|| dotenv.get(key).cloned())
        .unwrap_or_else(|| default.to_string())
}

fn env_val_opt(key: &str, dotenv: &HashMap<String, String>) -> Option<String> {
    env::var(key)
        .ok()
        .or_else(|| dotenv.get(key).cloned())
        .filter(|s| !s.is_empty())
}

fn env_val_usize(key: &str, dotenv: &HashMap<String, String>, default: usize) -> usize {
    match env_val_or(key, dotenv, &default.to_string()).parse::<usize>() {
        Ok(n) if n > 0 => n,
        Ok(_) => {
            log::warn!("{key}=0 is invalid, defaulting to {default}");
            default
        }
        Err(_) => {
            log::warn!("{key} is not a valid number, defaulting to {default}");
            default
        }
    }
}

fn parse_models(raw: &str) -> Result<Vec<ModelConfig>> {
    raw.split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|entry| {
            let entry = entry.trim();
            let (alias, path) = entry.split_once(':').with_context(|| {
                format!("invalid M2V_MODELS entry '{entry}' — expected <alias>:<path>")
            })?;
            let alias = alias.trim().to_string();
            let path = path.trim().to_string();

            validate_model_path(&path, &alias)?;

            Ok(ModelConfig { alias, path })
        })
        .collect()
}

fn validate_model_path(path: &str, alias: &str) -> Result<()> {
    if path.contains("..") {
        anyhow::bail!(
            "model path for '{alias}' contains '..' — path traversal is not allowed: {path}"
        );
    }

    if path.starts_with('/') {
        let mut normalized = std::path::PathBuf::new();
        for component in std::path::Path::new(path).components() {
            match component {
                std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                    normalized.push(component);
                }
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    let _ = normalized.pop();
                }
                std::path::Component::Normal(c) => {
                    normalized.push(c);
                }
            }
        }

        let canonical_str = normalized.to_string_lossy();
        // Append a trailing slash for prefix matching if it doesn't have one,
        // so that "/models_fake" doesn't match the prefix "/models/".
        let mut canonical_with_slash = canonical_str.to_string();
        if !canonical_with_slash.ends_with('/') {
            canonical_with_slash.push('/');
        }

        let allowed = ALLOWED_LOCAL_PREFIXES
            .iter()
            .any(|prefix| canonical_with_slash.starts_with(prefix));
        if !allowed {
            anyhow::bail!(
                "local path for '{alias}' must resolve under one of {ALLOWED_LOCAL_PREFIXES:?} — got: {path} (resolved: {canonical_str})"
            );
        }
    }

    Ok(())
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
    fn parse_local_path_allowed() {
        let models = parse_models("local:/models/my-model").unwrap();
        assert_eq!(models[0].path, "/models/my-model");
    }

    #[test]
    fn parse_local_path_opt_models() {
        let models = parse_models("local:/opt/models/my-model").unwrap();
        assert_eq!(models[0].path, "/opt/models/my-model");
    }

    #[test]
    fn parse_local_path_data_models() {
        let models = parse_models("local:/data/models/my-model").unwrap();
        assert_eq!(models[0].path, "/data/models/my-model");
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

    #[test]
    fn path_traversal_dotdot_rejected() {
        let result = parse_models("x:../etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("path traversal"), "got: {err}");
    }

    #[test]
    fn path_traversal_embedded_dotdot_rejected() {
        let result = parse_models("x:foo/../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn absolute_path_outside_allowed_rejected() {
        let result = parse_models("x:/etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("must resolve under"), "got: {err}");
    }

    #[test]
    fn absolute_path_under_models_allowed() {
        let models = parse_models("x:/models/sub/model").unwrap();
        assert_eq!(models[0].path, "/models/sub/model");
    }

    #[test]
    fn env_val_or_returns_env_var() {
        // std::env::set_var is unsafe in 1.80+ and we have #![forbid(unsafe_code)].
        // Just use an existing env var like CARGO or USER to test the 'env var exists' path.
        // We just need to make sure the function returns *something* from the environment.
        let key = if std::env::var("USER").is_ok() {
            "USER"
        } else {
            "PATH" // Almost guaranteed to exist
        };
        let expected = std::env::var(key).unwrap_or_default();
        let dotenv = HashMap::new();
        let val = env_val_or(key, &dotenv, "default_value");
        assert_eq!(val, expected);
    }

    #[test]
    fn env_val_or_returns_dotenv_when_no_env_var() {
        let mut dotenv = HashMap::new();
        let _ = dotenv.insert(
            "TEST_ENV_VAL_OR_VAR2".to_string(),
            "dotenv_value".to_string(),
        );
        let val = env_val_or("TEST_ENV_VAL_OR_VAR2", &dotenv, "default_value");
        assert_eq!(val, "dotenv_value");
    }

    #[test]
    fn env_val_or_returns_default_when_no_env_or_dotenv() {
        let dotenv = HashMap::new();
        let val = env_val_or("TEST_ENV_VAL_OR_VAR3", &dotenv, "default_value");
        assert_eq!(val, "default_value");
    }

    #[test]
    fn env_val_opt_returns_env_var() {
        let key = if std::env::var("USER").is_ok() {
            "USER"
        } else {
            "PATH"
        };
        let expected = std::env::var(key).unwrap_or_default();
        let dotenv = HashMap::new();
        let val = env_val_opt(key, &dotenv);
        assert_eq!(val, Some(expected));
    }

    #[test]
    fn env_val_opt_returns_dotenv_when_no_env_var() {
        let mut dotenv = HashMap::new();
        let _ = dotenv.insert(
            "TEST_ENV_VAL_OPT_VAR2".to_string(),
            "dotenv_value".to_string(),
        );
        let val = env_val_opt("TEST_ENV_VAL_OPT_VAR2", &dotenv);
        assert_eq!(val, Some("dotenv_value".to_string()));
    }

    #[test]
    fn env_val_opt_returns_none_when_empty_env_var() {
        // Can't reliably set an empty env var without unsafe.
        // We'll test with a variable we know doesn't exist.
        // The empty string logic is in the `filter(|s| !s.is_empty())` part of env_val_opt,
        // so we can test that by setting a dotenv value to "".
        let mut dotenv = HashMap::new();
        let _ = dotenv.insert("TEST_ENV_VAL_OPT_EMPTY".to_string(), String::new());
        let val = env_val_opt("TEST_ENV_VAL_OPT_EMPTY", &dotenv);
        assert_eq!(val, None);
    }

    #[test]
    fn env_val_opt_returns_none_when_no_env_or_dotenv() {
        let dotenv = HashMap::new();
        let val = env_val_opt("TEST_ENV_VAL_OPT_MISSING", &dotenv);
        assert_eq!(val, None);
    }
}
