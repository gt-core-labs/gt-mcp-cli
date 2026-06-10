//! `gt mcp call|list|resources|resource` — the shell MCP-client surface.
//!
//! Each op runs against the active `.gt-config` (auth + pre-flight refresh handled by the
//! caller) and prints the result as pretty JSON, so agents can drive tools from scripts —
//! the role the retired `gt-mcp-cli` played. `gt mcp` with no subcommand is the stdio proxy.

use anyhow::{Context, Result};
use serde_json::Value;

use crate::project_config::ProjectConfig;

/// `gt mcp call <tool> '<json-args>'` — `args` may be a JSON object string, `-` for stdin, or
/// omitted (no arguments).
pub async fn call(cfg: &ProjectConfig, tool: &str, args: Option<String>) -> Result<()> {
    // hq-rig-isolation.6: inject the rig from the active config into every tool call so
    // tools that accept `rig` (e.g. issues.list.execute) automatically scope to the right
    // rig without the caller having to pass it explicitly. Unknown tools ignore the field.
    let parsed = inject_rig(parse_args(args)?, &cfg.rig);
    let v = gt_mcp::invoke::call_tool(
        &cfg.server_url,
        &cfg.access_token,
        &cfg.workspace,
        tool,
        parsed,
    )
    .await?;
    print_json(&v)
}

/// Inject `rig` into the tool arguments when it is not already set. Absent or empty
/// config rig = no injection (back-compat). Caller-supplied `rig` in the args is preserved.
fn inject_rig(args: Option<Value>, rig: &str) -> Option<Value> {
    if rig.is_empty() {
        return args;
    }
    let mut obj = match args {
        Some(Value::Object(m)) => m,
        Some(other) => return Some(other), // non-object args: pass through unchanged
        None => serde_json::Map::new(),
    };
    obj.entry("rig").or_insert_with(|| Value::String(rig.to_string()));
    Some(Value::Object(obj))
}

/// `gt mcp list` — tools (name + description + input schema).
pub async fn list(cfg: &ProjectConfig) -> Result<()> {
    let v = gt_mcp::invoke::list_tools(&cfg.server_url, &cfg.access_token, &cfg.workspace).await?;
    print_json(&v)
}

/// `gt mcp resources` — available resources.
pub async fn resources(cfg: &ProjectConfig) -> Result<()> {
    let v =
        gt_mcp::invoke::list_resources(&cfg.server_url, &cfg.access_token, &cfg.workspace).await?;
    print_json(&v)
}

/// `gt mcp resource <uri>` — read one resource (e.g. `gt://issues?limit=10`).
pub async fn resource(cfg: &ProjectConfig, uri: &str) -> Result<()> {
    let v = gt_mcp::invoke::read_resource(&cfg.server_url, &cfg.access_token, &cfg.workspace, uri)
        .await?;
    print_json(&v)
}

/// Resolve tool arguments: `None` → no args; `"-"` → read a JSON object from stdin; otherwise
/// parse the string as JSON. The value must be a JSON object (the tool input schema).
fn parse_args(args: Option<String>) -> Result<Option<Value>> {
    let raw = match args {
        None => return Ok(None),
        Some(s) if s == "-" => {
            std::io::read_to_string(std::io::stdin()).context("read tool args from stdin")?
        }
        Some(s) => s,
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(&raw).context("tool args must be valid JSON")?;
    Ok(Some(v))
}

fn print_json(v: &Value) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(v).context("serialize result")?
    );
    Ok(())
}
