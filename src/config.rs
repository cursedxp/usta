//! Yapılandırma: API key + model + brain kökü çözümleme.

use anyhow::Result;

/// API key'i çevreden çöz. Saf fonksiyon — test edilebilir.
pub fn resolve_key(env_value: Option<String>) -> Result<String> {
    match env_value {
        Some(k) if !k.trim().is_empty() => Ok(k),
        _ => anyhow::bail!(
            "ANTHROPIC_API_KEY tanımlı değil. Şunu çalıştır:\n  export ANTHROPIC_API_KEY=sk-ant-..."
        ),
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
