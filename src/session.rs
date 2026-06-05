//! Pre-flight token refresh for the per-project MCP connection.
//!
//! Access tokens are short-lived RS256 JWTs, so a config saved by `gt init` minutes ago is
//! often already expired by the next `gt mcp`. Before opening the proxy we read the access
//! token's `exp` claim (no signature check — just to decide) and, if it is expired or within
//! [`REFRESH_SKEW_SECS`] of expiring, exchange the refresh token for a fresh pair at
//! `/auth/refresh` and persist it back to `.gt-config`. So the model never sees a
//! `token expired` from a stale config.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use base64::Engine;
use gt_mcp::Client;

use crate::project_config::{normalize_server_url, ConfigStore, ProjectConfig};

/// Refresh when the access token expires within this window (or is already expired / unreadable).
const REFRESH_SKEW_SECS: u64 = 120;

/// Load the active per-project config with a guaranteed-fresh access token. Used by every
/// `gt mcp` operation (proxy + call/list/resources/resource). Errors when no project is
/// connected, pointing at `gt init`.
pub async fn load_fresh() -> Result<ProjectConfig> {
    let store = ConfigStore::discover()?;
    let name = store.active_name()?.ok_or_else(|| {
        anyhow::anyhow!(
            "no active config in {} — run `gt init` first",
            store.dir().display()
        )
    })?;
    let mut cfg = store
        .get(&name)?
        .ok_or_else(|| anyhow::anyhow!("active config `{name}` is missing"))?;
    // Defensive: tolerate an older config that stored the /mcp endpoint as the base.
    cfg.server_url = normalize_server_url(&cfg.server_url);
    refresh_if_needed(&store, &name, cfg).await
}

/// Return `cfg` with a non-expiring access token, refreshing + persisting under `name` when the
/// current one is stale. A failed refresh is surfaced with a hint to re-run `gt init`.
pub async fn refresh_if_needed(
    store: &ConfigStore,
    name: &str,
    cfg: ProjectConfig,
) -> Result<ProjectConfig> {
    if !needs_refresh(&cfg.access_token) {
        return cfg_ok(cfg);
    }
    eprintln!("[gt] access token expired/expiring — refreshing …");
    let client = Client::new(&cfg.server_url)?;
    let tokens = client.refresh(&cfg.refresh_token).await.context(
        "refresh failed (the refresh token may have expired) — run `gt init` to log in again",
    )?;
    let fresh = ProjectConfig {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        ..cfg
    };
    store
        .save(name, &fresh, true)
        .context("persist the refreshed tokens to .gt-config")?;
    eprintln!("[gt] token refreshed and saved");
    Ok(fresh)
}

fn cfg_ok(cfg: ProjectConfig) -> Result<ProjectConfig> {
    Ok(cfg)
}

/// True when the JWT is expired, within [`REFRESH_SKEW_SECS`] of expiry, or its `exp` can't be
/// read (treat unreadable as "refresh" — safer than forwarding a token we can't reason about).
fn needs_refresh(access_token: &str) -> bool {
    match token_exp(access_token) {
        Some(exp) => now_secs().saturating_add(REFRESH_SKEW_SECS) >= exp,
        None => true,
    }
}

/// Read the `exp` (epoch seconds) from a JWT's payload WITHOUT verifying the signature — this is
/// only a heuristic to decide whether to refresh, never an auth decision.
fn token_exp(jwt: &str) -> Option<u64> {
    let payload_b64 = jwt.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("exp")?.as_u64()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt_with_exp(exp: u64) -> String {
        let payload = serde_json::json!({ "exp": exp }).to_string();
        let b = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
        format!("h.{b}.s")
    }

    #[test]
    fn expired_and_soon_need_refresh() {
        assert!(needs_refresh(&jwt_with_exp(now_secs().saturating_sub(10))));
        assert!(needs_refresh(&jwt_with_exp(now_secs() + 30))); // within skew
    }

    #[test]
    fn far_future_does_not() {
        assert!(!needs_refresh(&jwt_with_exp(now_secs() + 3600)));
    }

    #[test]
    fn unreadable_needs_refresh() {
        assert!(needs_refresh("garbage"));
        assert!(needs_refresh(""));
    }
}
