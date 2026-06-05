//! `gt prime` — session bootstrap / context report (hq-mt-cli.1).
//!
//! In the multi-tenant world every MCP call is scoped to a workspace, so a shell that drives
//! `gt` must declare which tenant it speaks for. `prime` is the guard: it **requires**
//! `GT_WORKSPACE` and aborts when it is unset, then reports the resolved workspace plus the
//! role/rig the shell carries.
//!
//! ## Resolution order
//!
//! `GT_WORKSPACE` (env) > `default_workspace` (config.toml, hq-mt-cli.3) > the legacy
//! `GT_WORKSPACE_DEFAULT_OPT_IN` grace fallback to `default` > abort. A configured default is a
//! real, named tenant, so it ranks above the catch-all grace `default` but below an explicit env
//! override.
//!
//! ## Grace period
//!
//! Legacy scripts predate the requirement, so `GT_WORKSPACE_DEFAULT_OPT_IN` lets a shell fall
//! back to the `default` workspace instead of aborting. It is opt-in on purpose: the abort is
//! the default so a missing tenant fails loud rather than silently writing to `default`.
//!
//! ## Scope (deferred)
//!
//! `role`/`rig` are read straight from `GT_ROLE`/`GT_RIG`. an upstream `gt prime` additionally
//! infers role from the cwd/town-root layout (`find_town_root`, `detect_role_from_cwd`); that
//! machinery is **not** ported here — it rides in with the wider `gt` CLI unification
//! (hq-mod-flags.5). Until then `prime` reports the env-declared identity only.

use crate::config::Config;
use serde_json::json;

/// Outcome of resolving the workspace from the environment.
enum Resolved {
    /// `GT_WORKSPACE` was set: the tenant the shell declared.
    Env(String),
    /// `GT_WORKSPACE` was unset; `default_workspace` from config.toml supplied the tenant.
    ConfigDefault(String),
    /// `GT_WORKSPACE` was unset but `GT_WORKSPACE_DEFAULT_OPT_IN` allowed the legacy fallback.
    GraceDefault,
    /// `GT_WORKSPACE` was unset and no opt-in: abort.
    Missing,
}

/// Resolve the active workspace. Empty strings are treated as unset (an exported-but-empty
/// `GT_WORKSPACE=` is a misconfiguration, not a tenant named "").
///
/// Precedence: env `GT_WORKSPACE` > config `default_workspace` > grace `default` opt-in > abort.
fn resolve_workspace(cfg: &Config) -> Resolved {
    if let Some(ws) = non_empty("GT_WORKSPACE") {
        return Resolved::Env(ws);
    }
    if let Some(ws) = cfg.default_workspace.as_deref().filter(|s| !s.is_empty()) {
        return Resolved::ConfigDefault(ws.to_string());
    }
    if opt_in_enabled() {
        return Resolved::GraceDefault;
    }
    Resolved::Missing
}

/// A non-empty environment variable, or `None`.
fn non_empty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// Whether the legacy `default`-workspace fallback is opted into. Any non-empty value enables
/// it except the explicit negatives `0`/`false`/`no` (so `=1`, `=true`, or a bare presence all
/// work, while `=0` keeps the abort).
fn opt_in_enabled() -> bool {
    match non_empty("GT_WORKSPACE_DEFAULT_OPT_IN") {
        Some(v) => !matches!(v.to_ascii_lowercase().as_str(), "0" | "false" | "no"),
        None => false,
    }
}

/// Run `gt prime`. Returns the process exit code: `0` on a resolved workspace, `1` when
/// `GT_WORKSPACE` is unset and the grace opt-in is absent.
pub fn run(json: bool) -> i32 {
    let cfg = Config::load();
    let (workspace, source) = match resolve_workspace(&cfg) {
        Resolved::Env(ws) => (ws, "env"),
        Resolved::ConfigDefault(ws) => (ws, "config-default"),
        Resolved::GraceDefault => ("default".to_string(), "grace-default"),
        Resolved::Missing => {
            abort_missing();
            return 1;
        }
    };

    let role = non_empty("GT_ROLE").unwrap_or_else(|| "unknown".to_string());
    let rig = non_empty("GT_RIG");

    if json {
        let _ = println!(
            "{}",
            json!({
                "workspace": workspace,
                "source": source,
                "role": role,
                "rig": rig,
            })
        );
    } else {
        println!("# gt — workspace context");
        println!();
        println!("Workspace: {workspace}{}", grace_note(source));
        println!("Role:      {role}");
        if let Some(r) = &rig {
            println!("Rig:       {r}");
        }
    }
    0
}

/// The trailing note shown after a non-env workspace in text mode, explaining where it came from.
fn grace_note(source: &str) -> &'static str {
    match source {
        "config-default" => "  (GT_WORKSPACE unset — config default_workspace)",
        "grace-default" => "  (GT_WORKSPACE unset — legacy GT_WORKSPACE_DEFAULT_OPT_IN fallback)",
        _ => "",
    }
}

/// Print the abort guidance to stderr. Kept separate so the message is testable and the
/// `run` control flow stays a straight `return 1`.
fn abort_missing() {
    eprintln!("error: GT_WORKSPACE is not set — every gt command is scoped to a workspace.");
    eprintln!();
    eprintln!("Set the tenant this shell speaks for:");
    eprintln!("    export GT_WORKSPACE=<your-workspace>");
    eprintln!();
    eprintln!("Or set a persistent default in ~/.config/gt/config.toml:");
    eprintln!("    default_workspace = \"<your-workspace>\"");
    eprintln!();
    eprintln!("Legacy scripts may opt into the `default` workspace instead:");
    eprintln!("    export GT_WORKSPACE_DEFAULT_OPT_IN=1");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env is process-global; serialize the tests that mutate it.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear() {
        std::env::remove_var("GT_WORKSPACE");
        std::env::remove_var("GT_WORKSPACE_DEFAULT_OPT_IN");
    }

    /// A config with no defaults — isolates env-only precedence tests from any on-disk file.
    fn no_cfg() -> Config {
        Config::default()
    }

    /// A config carrying a `default_workspace`.
    fn cfg_ws(ws: &str) -> Config {
        Config {
            default_workspace: Some(ws.to_string()),
            ..Config::default()
        }
    }

    #[test]
    fn env_workspace_resolves() {
        let _g = ENV_LOCK.lock().unwrap();
        clear();
        std::env::set_var("GT_WORKSPACE", "acme");
        assert!(matches!(resolve_workspace(&no_cfg()), Resolved::Env(ws) if ws == "acme"));
        clear();
    }

    #[test]
    fn empty_workspace_is_unset() {
        let _g = ENV_LOCK.lock().unwrap();
        clear();
        std::env::set_var("GT_WORKSPACE", "");
        assert!(matches!(resolve_workspace(&no_cfg()), Resolved::Missing));
        clear();
    }

    #[test]
    fn missing_without_opt_in_aborts() {
        let _g = ENV_LOCK.lock().unwrap();
        clear();
        assert!(matches!(resolve_workspace(&no_cfg()), Resolved::Missing));
        clear();
    }

    #[test]
    fn opt_in_falls_back_to_default() {
        let _g = ENV_LOCK.lock().unwrap();
        clear();
        std::env::set_var("GT_WORKSPACE_DEFAULT_OPT_IN", "1");
        assert!(matches!(resolve_workspace(&no_cfg()), Resolved::GraceDefault));
        clear();
    }

    #[test]
    fn opt_in_negative_value_still_aborts() {
        let _g = ENV_LOCK.lock().unwrap();
        clear();
        std::env::set_var("GT_WORKSPACE_DEFAULT_OPT_IN", "0");
        assert!(matches!(resolve_workspace(&no_cfg()), Resolved::Missing));
        clear();
    }

    #[test]
    fn explicit_workspace_wins_over_opt_in() {
        let _g = ENV_LOCK.lock().unwrap();
        clear();
        std::env::set_var("GT_WORKSPACE", "acme");
        std::env::set_var("GT_WORKSPACE_DEFAULT_OPT_IN", "1");
        assert!(matches!(resolve_workspace(&no_cfg()), Resolved::Env(ws) if ws == "acme"));
        clear();
    }

    #[test]
    fn config_default_used_when_env_unset() {
        let _g = ENV_LOCK.lock().unwrap();
        clear();
        assert!(matches!(
            resolve_workspace(&cfg_ws("beta")),
            Resolved::ConfigDefault(ws) if ws == "beta"
        ));
        clear();
    }

    #[test]
    fn env_workspace_wins_over_config_default() {
        let _g = ENV_LOCK.lock().unwrap();
        clear();
        std::env::set_var("GT_WORKSPACE", "acme");
        assert!(matches!(
            resolve_workspace(&cfg_ws("beta")),
            Resolved::Env(ws) if ws == "acme"
        ));
        clear();
    }

    #[test]
    fn config_default_wins_over_grace_opt_in() {
        let _g = ENV_LOCK.lock().unwrap();
        clear();
        std::env::set_var("GT_WORKSPACE_DEFAULT_OPT_IN", "1");
        assert!(matches!(
            resolve_workspace(&cfg_ws("beta")),
            Resolved::ConfigDefault(ws) if ws == "beta"
        ));
        clear();
    }
}
