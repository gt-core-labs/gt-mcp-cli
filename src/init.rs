//! `gt init` — the first-run MCP-connection wizard.
//!
//! Logs in to a gt-core server, lists its workspaces and rigs, lets the user pick one of
//! each, and persists the result as a named config under `.gt-config/` (marked active).
//! Every prompt has a matching flag, so the same command runs unattended in CI: when all
//! of `--server/--email/--password/--workspace/--rig` are supplied no prompt is shown,
//! and `--yes` turns a still-missing value into an error rather than a hang.
//!
//! The REST + MCP wire logic lives in the `gt-mcp` crate; this module is only the UX.

use anyhow::{bail, Context, Result};
use gt_mcp::Client;

use crate::project_config::{ConfigStore, ProjectConfig};

/// Parsed `gt init` flags. Any `None` is resolved by prompting, unless `no_interactive`.
#[derive(Debug, Default)]
pub struct InitArgs {
    pub server: Option<String>,
    pub email: Option<String>,
    pub password: Option<String>,
    pub workspace: Option<String>,
    pub rig: Option<String>,
    pub name: Option<String>,
    /// Fail instead of prompting for anything still missing (CI / scripts).
    pub no_interactive: bool,
}

const DEFAULT_SERVER: &str = "http://127.0.0.1:8765";

pub async fn run(args: InitArgs) -> Result<()> {
    let store = ConfigStore::discover()?;

    let server = match args.server {
        Some(s) => s,
        None => prompt_text(args.no_interactive, "Server URL", Some(DEFAULT_SERVER))?,
    };
    let client = Client::new(&server)?;

    let email = match args.email {
        Some(e) => e,
        None => prompt_text(args.no_interactive, "Email", None)?,
    };
    let password = match args.password {
        Some(p) => p,
        None => prompt_password(args.no_interactive)?,
    };

    eprintln!("[gt init] logging in to {server} …");
    let tokens = client.login(&email, &password).await?;

    // Workspace: offer the catalog; a flag short-circuits the menu but is still
    // validated against the catalog so a typo fails here, not at first use.
    let workspaces = client.list_workspaces(&tokens.access_token).await?;
    if workspaces.is_empty() {
        bail!("the server returned no workspaces for this account");
    }
    let workspace = match args.workspace {
        Some(w) => {
            if !workspaces.iter().any(|x| x.id == w) {
                bail!(
                    "workspace `{w}` not in the catalog; available: {}",
                    workspaces.iter().map(|x| x.id.as_str()).collect::<Vec<_>>().join(", ")
                );
            }
            w
        }
        None => {
            let labels: Vec<String> = workspaces
                .iter()
                .map(|w| format!("{}  ({}, {})", w.id, w.name, w.status))
                .collect();
            let idx = prompt_select(args.no_interactive, "Workspace", &labels)?;
            workspaces[idx].id.clone()
        }
    };

    // Rig: scoped to the chosen workspace.
    let rigs = client.list_rigs(&tokens.access_token, &workspace).await?;
    if rigs.is_empty() {
        bail!("workspace `{workspace}` has no rigs; register one before `gt init`");
    }
    let rig = match args.rig {
        Some(r) => rigs
            .iter()
            .find(|x| x.name == r || x.prefix == r)
            .map(|x| x.prefix.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "rig `{r}` not in workspace `{workspace}`; available: {}",
                    rigs.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", ")
                )
            })?,
        None => {
            let labels: Vec<String> = rigs
                .iter()
                .map(|r| format!("{}  (prefix {})", r.name, r.prefix))
                .collect();
            let idx = prompt_select(args.no_interactive, "Rig", &labels)?;
            rigs[idx].prefix.clone()
        }
    };

    let name = match args.name {
        Some(n) => n,
        None => prompt_text(args.no_interactive, "Config name", Some(&workspace))?,
    };

    let cfg = ProjectConfig {
        server_url: server,
        workspace: workspace.clone(),
        rig: rig.clone(),
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
    };
    store.save(&name, &cfg, true)?;

    eprintln!(
        "[gt init] saved config `{name}` (workspace={workspace}, rig={rig}) → {}",
        store.dir().display()
    );
    eprintln!("[gt init] `.gt-config/` is git-ignored; `gt mcp` will use this config.");
    Ok(())
}

fn prompt_text(no_interactive: bool, label: &str, default: Option<&str>) -> Result<String> {
    if no_interactive {
        bail!("missing value for `{label}` (non-interactive: pass the matching flag)");
    }
    let prompt = format!("{label}:");
    let mut t = inquire::Text::new(&prompt);
    if let Some(d) = default {
        t = t.with_default(d);
    }
    t.prompt().with_context(|| format!("prompt {label}"))
}

fn prompt_password(no_interactive: bool) -> Result<String> {
    if no_interactive {
        bail!("missing value for `Password` (non-interactive: pass --password)");
    }
    inquire::Password::new("Password:")
        .without_confirmation()
        .prompt()
        .context("prompt password")
}

fn prompt_select(no_interactive: bool, label: &str, options: &[String]) -> Result<usize> {
    if no_interactive {
        bail!("missing value for `{label}` (non-interactive: pass the matching flag)");
    }
    let choice = inquire::Select::new(&format!("{label}:"), options.to_vec())
        .prompt()
        .with_context(|| format!("select {label}"))?;
    Ok(options.iter().position(|o| o == &choice).unwrap_or(0))
}
