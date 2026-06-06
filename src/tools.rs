//! `gt tools` — expose gt's OWN subcommands as MCP tools over stdio.
//!
//! So a model can discover and drive the operator CLI as native tools (alongside the
//! orchestrator tools that `gt mcp` proxies). Each tool shells out to this same binary
//! (`current_exe`), so behaviour is identical to the CLI — no logic is duplicated. The
//! server advertises a `tools` capability and ships `instructions` describing the surface,
//! and every tool carries a description, so `tools/list` alone tells the model what exists.
//!
//! Register it for an agent with `gt register` (writes `.mcp.json`).

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo};
use rmcp::transport::io::stdio;
use rmcp::{serve_server, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

const INSTRUCTIONS: &str = "\
The gt-core client CLI exposed as tools. gt_prime reports the shell's workspace/role/rig; \
gt_config_* manage the per-project MCP connection saved by `gt init`; gt_workspace_use prints \
an export line; gt_update_check peeks for a newer release. gt_help prints the full CLI help.";

#[derive(Clone)]
pub struct GtTools {
    // Read by the `#[tool_handler]`-generated dispatch, not by hand.
    #[allow(dead_code)]
    tool_router: ToolRouter<GtTools>,
}

#[derive(Deserialize, JsonSchema, Default)]
struct PrimeArgs {
    /// Emit the context as JSON instead of the text report.
    #[serde(default)]
    json: bool,
}

#[derive(Deserialize, JsonSchema)]
struct WorkspaceUseArgs {
    /// Workspace id to select.
    id: String,
}

#[tool_router]
impl GtTools {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "gt_help",
        description = "Print the full `gt` CLI help (every command + flags)."
    )]
    fn gt_help(&self) -> CallToolResult {
        run_gt(&["--help"])
    }

    #[tool(
        name = "gt_prime",
        description = "Report the active workspace/role/rig the shell carries (reads env only)."
    )]
    fn gt_prime(&self, Parameters(a): Parameters<PrimeArgs>) -> CallToolResult {
        if a.json {
            run_gt(&["prime", "--json"])
        } else {
            run_gt(&["prime"])
        }
    }

    #[tool(
        name = "gt_config_list",
        description = "List the per-project named MCP configs under .gt-config/ (active marked *)."
    )]
    fn gt_config_list(&self) -> CallToolResult {
        run_gt(&["config", "list"])
    }

    #[tool(
        name = "gt_config_show",
        description = "Show the active per-project MCP config (server/workspace/rig; tokens redacted)."
    )]
    fn gt_config_show(&self) -> CallToolResult {
        run_gt(&["config", "show"])
    }

    #[tool(
        name = "gt_workspace_use",
        description = "Print the `export GT_WORKSPACE=<id>` line for a workspace (for shell eval)."
    )]
    fn gt_workspace_use(&self, Parameters(a): Parameters<WorkspaceUseArgs>) -> CallToolResult {
        run_gt(&["workspace", "use", &a.id])
    }

    #[tool(
        name = "gt_update_check",
        description = "Check whether a newer `gt` release exists (does not download)."
    )]
    fn gt_update_check(&self) -> CallToolResult {
        run_gt(&["update", "--check"])
    }
}

#[tool_handler]
impl ServerHandler for GtTools {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        info.server_info = Implementation::from_build_env();
        info.instructions = Some(INSTRUCTIONS.to_string());
        info
    }
}

/// Run a `gt` subcommand by re-invoking THIS binary, returning its combined output as the
/// tool result (error result on a non-zero exit). Disables the passive update check so the
/// notice never leaks into a tool's output.
fn run_gt(args: &[&str]) -> CallToolResult {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("gt"));
    match Command::new(&exe)
        .args(args)
        .env("GT_NO_UPDATE_CHECK", "1")
        .output()
    {
        Ok(out) => {
            let mut body = String::from_utf8_lossy(&out.stdout).into_owned();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(&err);
            }
            if body.trim().is_empty() {
                body = format!("({})", out.status);
            }
            if out.status.success() {
                CallToolResult::success(vec![Content::text(body)])
            } else {
                CallToolResult::error(vec![Content::text(body)])
            }
        }
        Err(e) => CallToolResult::error(vec![Content::text(format!("failed to exec gt: {e}"))]),
    }
}

/// Serve the gt-tools MCP server over stdio until the peer closes.
pub async fn run() -> Result<()> {
    let server = GtTools::new();
    let running = serve_server(server, stdio())
        .await
        .context("serve the gt-tools stdio MCP transport")?;
    running.waiting().await.context("gt-tools serve loop")?;
    Ok(())
}
