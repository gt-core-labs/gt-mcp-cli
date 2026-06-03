//! `~/.config/gastown/config.toml` — persistent CLI defaults (hq-mt-cli.3).
//!
//! One setting the shell would otherwise have to pass on every invocation:
//!
//! - `default_workspace` — the tenant to assume when `GT_WORKSPACE` is unset. It sits *below*
//!   the env var in precedence (an explicit `GT_WORKSPACE` always wins) but *above* the legacy
//!   `GT_WORKSPACE_DEFAULT_OPT_IN` grace fallback, so a configured default is a real, named
//!   tenant rather than the catch-all `default`.
//!
//! The file is optional: a missing file, an unreadable file, or a parse error all degrade to
//! "no config" (an empty [`Config`]) rather than aborting. A malformed file is reported once on
//! stderr so the misconfiguration is not silent.

use serde::Deserialize;

/// Parsed `config.toml`. Every field is optional so a partial or absent file is valid.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// Tenant to assume when `GT_WORKSPACE` is unset.
    pub default_workspace: Option<String>,
}

impl Config {
    /// Load `$XDG_CONFIG_HOME/gastown/config.toml` (or `~/.config/gastown/config.toml`).
    ///
    /// Never fails: a missing file or a parse error yields [`Config::default`] so the caller can
    /// treat config as a pure set of optional overrides. A parse error is logged to stderr.
    pub fn load() -> Config {
        let Some(path) = config_path() else {
            return Config::default();
        };
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            // Absent (or unreadable) config is the common case, not an error.
            Err(_) => return Config::default(),
        };
        match toml::from_str(&raw) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("warning: ignoring malformed {}: {e}", path.display());
                Config::default()
            }
        }
    }
}

/// Resolve the config-file path: `$XDG_CONFIG_HOME/gastown/config.toml`, falling back to
/// `$HOME/.config/gastown/config.toml`. Returns `None` only when neither env var is set.
fn config_path() -> Option<std::path::PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
        return Some(std::path::PathBuf::from(xdg).join("gastown/config.toml"));
    }
    let home = std::env::var_os("HOME").filter(|s| !s.is_empty())?;
    Some(std::path::PathBuf::from(home).join(".config/gastown/config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_parses_to_defaults() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.default_workspace.is_none());
    }

    #[test]
    fn workspace_parses() {
        let cfg: Config = toml::from_str(r#"default_workspace = "acme""#).unwrap();
        assert_eq!(cfg.default_workspace.as_deref(), Some("acme"));
    }
}
