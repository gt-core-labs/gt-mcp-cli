//! `gt quota onboard` — host-side claude-account onboarding (`hq-quota-accounts.7`).
//!
//! The operator should not hand-pick an account id or type a credentials path. This drives the
//! full flow on the host (claude is NOT in the mcp-server container, so the login must run where
//! `claude` lives):
//!
//! 1. allocate a generic `CLAUDE_CONFIG_DIR` (`--dir` to override),
//! 2. `claude auth login` into it — interactive (the operator pastes the OOB code),
//! 3. `claude auth status --json` → the account identity (`email`) comes from the handshake,
//! 4. register it (`quota.register.execute {account: email, config_dir}`) over MCP, which emits the
//!    event-sourced `quota.account_registered.v1` the daemon hydrates its keychain from.
//!
//! The account id is the login's `email` — captured, never typed. `--account` overrides it (e.g.
//! to use the org id) and `--dir` pins the credentials dir (e.g. under the daemon's shared volume).

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::json;

use crate::project_config::ProjectConfig;

/// The `claude` binary; `GT_CLAUDE_BIN` overrides (it may live at `~/.local/bin/claude`, off the
/// daemon's PATH).
fn claude_bin() -> String {
    std::env::var("GT_CLAUDE_BIN")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "claude".to_string())
}

/// Resolve the credentials dir: `--dir` when given, else a generic per-onboarding path under
/// `$HOME/.claude-accounts/<unix-secs>`. The id is NOT the dir — the dir is just storage; the
/// account id comes from the handshake.
fn resolve_dir(dir: Option<String>) -> Result<PathBuf> {
    if let Some(d) = dir.filter(|d| !d.trim().is_empty()) {
        return Ok(PathBuf::from(d.trim()));
    }
    let home = std::env::var("HOME").context("HOME unset; pass --dir")?;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(PathBuf::from(home)
        .join(".claude-accounts")
        .join(secs.to_string()))
}

/// Run `claude` with `CLAUDE_CONFIG_DIR` pinned. `interactive` inherits the terminal (for the OOB
/// login paste); otherwise stdout is captured and returned.
fn run_claude(dir: &std::path::Path, args: &[&str], interactive: bool) -> Result<String> {
    let mut cmd = Command::new(claude_bin());
    cmd.args(args).env("CLAUDE_CONFIG_DIR", dir);
    if interactive {
        let status = cmd
            .status()
            .with_context(|| format!("spawn `{} {}`", claude_bin(), args.join(" ")))?;
        if !status.success() {
            bail!("`claude {}` exited with {status}", args.join(" "));
        }
        Ok(String::new())
    } else {
        let out = cmd
            .output()
            .with_context(|| format!("spawn `{} {}`", claude_bin(), args.join(" ")))?;
        if !out.status.success() {
            bail!(
                "`claude {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

/// Drive the onboarding flow and register the account over MCP.
pub async fn onboard(
    cfg: &ProjectConfig,
    dir: Option<String>,
    account_override: Option<String>,
) -> Result<()> {
    let dir = resolve_dir(dir)?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create credentials dir {}", dir.display()))?;
    eprintln!("[gt quota onboard] credentials dir: {}", dir.display());

    // 1) Interactive login (the operator pastes the OOB code).
    eprintln!("[gt quota onboard] launching `claude auth login` — log in with the account to add…");
    run_claude(&dir, &["auth", "login"], true)?;

    // 2) Identity from the handshake.
    let status_raw = run_claude(&dir, &["auth", "status", "--json"], false)?;
    let status: serde_json::Value = serde_json::from_str(status_raw.trim())
        .with_context(|| format!("parse `claude auth status --json`: {status_raw}"))?;
    if status.get("loggedIn").and_then(|v| v.as_bool()) != Some(true) {
        bail!("login did not complete (claude auth status: loggedIn != true)");
    }
    let account = match account_override.filter(|a| !a.trim().is_empty()) {
        Some(a) => a.trim().to_string(),
        None => status
            .get("email")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("no `email` in claude auth status; pass --account"))?,
    };
    eprintln!("[gt quota onboard] account from handshake: {account}");

    // 3) Register over MCP (event-sourced quota.account_registered.v1).
    let v = gt_mcp::invoke::call_tool(
        &cfg.server_url,
        &cfg.access_token,
        &cfg.workspace,
        "quota.register.execute",
        Some(json!({ "account": account, "config_dir": dir.display().to_string() })),
    )
    .await
    .context("register the account over MCP (needs quota.write)")?;

    println!(
        "{}",
        serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
    );
    eprintln!("[gt quota onboard] registered {account} → predictive rotation pool");
    Ok(())
}
