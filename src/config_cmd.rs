//! `gt config` — manage the per-project named configs (hq-gt-cli.3).
//!
//! - `gt config list`      — every named config, the active one marked `*`
//! - `gt config use <name>`— flip the active pointer
//! - `gt config show`      — print the active config (token redacted)

use anyhow::{bail, Result};

use crate::project_config::ConfigStore;

pub fn list() -> Result<()> {
    let store = ConfigStore::discover()?;
    let names = store.list()?;
    if names.is_empty() {
        eprintln!("no configs in {} — run `gt init`", store.dir().display());
        return Ok(());
    }
    let active = store.active_name()?;
    for name in names {
        let mark = if Some(&name) == active.as_ref() {
            "*"
        } else {
            " "
        };
        println!("{mark} {name}");
    }
    Ok(())
}

pub fn use_config(name: &str) -> Result<()> {
    let store = ConfigStore::discover()?;
    store.set_active(name)?;
    eprintln!("active config → {name}");
    Ok(())
}

pub fn show() -> Result<()> {
    let store = ConfigStore::discover()?;
    let Some(name) = store.active_name()? else {
        bail!("no active config — run `gt init` or `gt config use <name>`");
    };
    let cfg = store
        .get(&name)?
        .ok_or_else(|| anyhow::anyhow!("active pointer references missing config `{name}`"))?;
    // Tokens are secrets even though the file is git-ignored — never echo them.
    println!("name:       {name}");
    println!("server_url: {}", cfg.server_url);
    println!("workspace:  {}", cfg.workspace);
    println!("rig:        {}", cfg.rig);
    println!("role:       {}", cfg.role.as_deref().unwrap_or("(none)"));
    println!(
        "access_token:  <redacted, {} chars>",
        cfg.access_token.len()
    );
    println!("refresh_token: <redacted>");
    Ok(())
}
