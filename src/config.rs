//! Yapılandırma: API key + model + brain kökü çözümleme.

use anyhow::Result;
use std::path::PathBuf;

use crate::anthropic::DEFAULT_MODEL;

pub struct Config {
    pub api_key: String,
    pub model: String,
    pub root: PathBuf,
}

/// API key'i çevreden çöz. Saf fonksiyon — test edilebilir.
pub fn resolve_key(env_value: Option<String>) -> Result<String> {
    match env_value {
        Some(k) if !k.trim().is_empty() => Ok(k),
        _ => anyhow::bail!(
            "ANTHROPIC_API_KEY tanımlı değil. Şunu çalıştır:\n  export ANTHROPIC_API_KEY=sk-ant-..."
        ),
    }
}

impl Config {
    pub fn load(root: PathBuf) -> Result<Config> {
        let api_key = resolve_key(std::env::var("ANTHROPIC_API_KEY").ok())?;
        Ok(Config { api_key, model: DEFAULT_MODEL.to_string(), root })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_key_ok_when_set() {
        assert_eq!(resolve_key(Some("sk-ant-x".into())).unwrap(), "sk-ant-x");
    }

    #[test]
    fn resolve_key_errors_when_missing_or_blank() {
        assert!(resolve_key(None).is_err());
        assert!(resolve_key(Some("   ".into())).is_err());
    }
}
