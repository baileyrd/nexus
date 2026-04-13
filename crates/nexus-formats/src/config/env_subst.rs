//! Environment variable substitution for config files.
//!
//! Replaces `${VAR_NAME}` placeholders in raw text with the corresponding
//! environment variable values **before** TOML / JSON deserialization.

use regex_lite::Regex;
use std::sync::OnceLock;

use crate::error::ConfigError;

static ENV_VAR_RE: OnceLock<Regex> = OnceLock::new();

fn env_var_re() -> &'static Regex {
    ENV_VAR_RE.get_or_init(|| {
        Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").expect("static regex is valid")
    })
}

/// Replace all `${VAR_NAME}` placeholders in `text` with environment variable values.
///
/// Returns the substituted string on success. Returns
/// [`ConfigError::UndefinedEnvVar`] for the **first** placeholder whose
/// variable is not defined in the process environment.
///
/// # Errors
///
/// Returns [`ConfigError::UndefinedEnvVar`] if any referenced variable is unset.
pub fn substitute(text: &str) -> Result<String, ConfigError> {
    let re = env_var_re();
    let mut error: Option<ConfigError> = None;

    let result = re.replace_all(text, |caps: &regex_lite::Captures<'_>| {
        if error.is_some() {
            return String::new();
        }
        let name = &caps[1];
        if let Ok(val) = std::env::var(name) {
            val
        } else {
            error = Some(ConfigError::UndefinedEnvVar { name: name.to_string() });
            String::new()
        }
    });

    if let Some(err) = error {
        return Err(err);
    }

    Ok(result.into_owned())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_placeholders_passthrough() {
        let text = "plain text without any placeholders";
        assert_eq!(substitute(text).unwrap(), text);
    }

    #[test]
    fn single_var_substituted() {
        std::env::set_var("_NEXUS_TEST_VAR", "hello_value");
        let result = substitute("key = \"${_NEXUS_TEST_VAR}\"").unwrap();
        assert!(result.contains("hello_value"));
        assert!(!result.contains("${_NEXUS_TEST_VAR}"));
        std::env::remove_var("_NEXUS_TEST_VAR");
    }

    #[test]
    fn multiple_vars_substituted() {
        std::env::set_var("_NEXUS_A", "alpha");
        std::env::set_var("_NEXUS_B", "beta");
        let result = substitute("${_NEXUS_A} and ${_NEXUS_B}").unwrap();
        assert_eq!(result, "alpha and beta");
        std::env::remove_var("_NEXUS_A");
        std::env::remove_var("_NEXUS_B");
    }

    #[test]
    fn undefined_var_returns_error() {
        // Use a name that is almost certainly not set.
        let result = substitute("${_NEXUS_DEFINITELY_NOT_SET_XYZZY}");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ConfigError::UndefinedEnvVar { .. }));
    }

    #[test]
    fn identity_when_no_match() {
        let text = r#"api_key = "literal-no-placeholder""#;
        assert_eq!(substitute(text).unwrap(), text);
    }
}
