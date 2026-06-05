//! `gt register` / `gt unregister` — (de)register gt as MCP servers in a client config.
//!
//! Installs two stdio MCP servers so a model discovers them automatically:
//!   - `gt`       → `gt mcp`   (proxy to the orchestrator's tools, authenticated)
//!   - `gt-tools` → `gt tools` (gt's own subcommands as tools)
//!
//! Default target is the project file `./.mcp.json` (the portable, shareable MCP config).
//! `--global` targets `~/.claude.json`'s top-level `mcpServers` instead. Existing entries
//! and unrelated keys are preserved; only the `gt`/`gt-tools` keys are added or removed.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{json, Value};

const SERVERS: [(&str, &str); 2] = [("gt", "mcp"), ("gt-tools", "tools")];

pub fn run(global: bool, remove: bool) -> Result<()> {
    let path = target_path(global)?;
    let mut root = read_json(&path)?;

    // Ensure `root` is an object and grab/insert its `mcpServers` map.
    let obj = root
        .as_object_mut()
        .context("config root is not a JSON object")?;
    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("`mcpServers` is not a JSON object")?;

    if remove {
        let mut removed = Vec::new();
        for (name, _) in SERVERS {
            if servers.remove(name).is_some() {
                removed.push(name);
            }
        }
        write_json(&path, &root)?;
        if removed.is_empty() {
            eprintln!("gt: no gt entries in {}", path.display());
        } else {
            eprintln!("gt: unregistered {:?} from {}", removed, path.display());
        }
        return Ok(());
    }

    let exe = std::env::current_exe()
        .context("resolve the gt executable path")?
        .to_string_lossy()
        .into_owned();
    for (name, sub) in SERVERS {
        servers.insert(
            name.to_string(),
            json!({ "command": exe, "args": [sub] }),
        );
    }
    write_json(&path, &root)?;
    eprintln!(
        "gt: registered `gt` (mcp proxy) + `gt-tools` in {}",
        path.display()
    );
    eprintln!("gt: restart the MCP client (e.g. Claude Code) to pick up the change.");
    if SERVERS.iter().any(|(n, _)| *n == "gt") {
        eprintln!("gt: the `gt` proxy needs an active config — run `gt init` if you haven't.");
    }
    Ok(())
}

fn target_path(global: bool) -> Result<PathBuf> {
    if global {
        let home = std::env::var_os("HOME").context("HOME is unset")?;
        Ok(PathBuf::from(home).join(".claude.json"))
    } else {
        Ok(std::env::current_dir()
            .context("resolve current directory")?
            .join(".mcp.json"))
    }
}

/// Read a JSON config, or an empty object when the file does not exist.
fn read_json(path: &PathBuf) -> Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(raw) if raw.trim().is_empty() => Ok(json!({})),
        Ok(raw) => serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

fn write_json(path: &PathBuf, value: &Value) -> Result<()> {
    let body = serde_json::to_string_pretty(value).context("serialize config")?;
    std::fs::write(path, body + "\n").with_context(|| format!("write {}", path.display()))?;
    Ok(())
}
