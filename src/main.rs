//! `gt-mcp-cli` — a command-line client for the gt-mcp server.
//!
//! Speaks the Model Context Protocol over the streamable-HTTP transport using the official
//! `rmcp` SDK (the same SDK gt-mcp serves with), so every call goes through a real MCP
//! handshake — list/inspect tools, call them, and read the domain snapshot resources.

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use rmcp::model::{CallToolRequestParams, ReadResourceRequestParams};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use serde_json::{Map, Value};

mod prime;
mod workspace_cmd;

use workspace_cmd::WorkspaceAction;

#[derive(Parser)]
#[command(name = "gt-mcp-cli", version, about = "CLI client for the gt-mcp server")]
struct Cli {
    /// MCP endpoint URL (streamable HTTP).
    #[arg(
        long,
        env = "GT_MCP_URL",
        default_value = "http://127.0.0.1:8765/mcp",
        global = true
    )]
    url: String,

    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List available tools. Names + descriptions by default; `--full` dumps input schemas.
    Tools {
        #[arg(long)]
        full: bool,
    },
    /// List available resources (the domain snapshot URIs).
    Resources,
    /// Call a tool. Pass arguments with repeated `--arg k=v` (each value is parsed as JSON,
    /// falling back to a string) or supply a whole object with `--json '{...}'`.
    Call {
        /// Tool name, e.g. `agent.add.execute`.
        name: String,
        #[arg(long = "arg", value_name = "K=V")]
        args: Vec<String>,
        /// Raw JSON object for the arguments (overrides any `--arg`).
        #[arg(long)]
        json: Option<String>,
    },
    /// Read a resource by URI, e.g. `gt://agent/sessions`.
    Read { uri: String },
    /// Report the active workspace/role/rig. Requires `GT_WORKSPACE` (aborts when unset unless
    /// `GT_WORKSPACE_DEFAULT_OPT_IN` opts into the legacy `default` fallback). Offline: reads the
    /// environment only, never contacts the server.
    Prime {
        /// Emit the context as a JSON object instead of the text report.
        #[arg(long)]
        json: bool,
    },
    /// Manage workspaces: `list` / `create` / `info` (over MCP) and `use` (prints an
    /// `export GT_WORKSPACE=<id>` line to eval).
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // `prime` is offline — it inspects the environment and never opens an MCP session, so it
    // short-circuits before the transport connect (which would otherwise require a live server).
    if let Command::Prime { json } = cli.cmd {
        std::process::exit(prime::run(json));
    }

    // `workspace use` is offline too — it only prints an `export` line for the shell to eval,
    // so it must not require a live server.
    if let Command::Workspace { action } = &cli.cmd {
        if action.run_offline() {
            return Ok(());
        }
    }

    let transport = StreamableHttpClientTransport::from_uri(cli.url.clone());
    let client = ()
        .serve(transport)
        .await
        .with_context(|| format!("connect + MCP initialize at {}", cli.url))?;

    // `RunningService` derefs to `Peer<RoleClient>`, so the request methods are called
    // directly on `client`. Run the command, then close the session cleanly.
    let mut exit_error = false;
    let outcome: Result<()> = async {
        match cli.cmd {
            Command::Tools { full } => {
                let tools = client.list_all_tools().await.context("list tools")?;
                if full {
                    println!("{}", serde_json::to_string_pretty(&tools)?);
                } else {
                    for t in &tools {
                        let desc = t.description.as_deref().unwrap_or("");
                        println!("{}\t{}", t.name, desc);
                    }
                }
            }
            Command::Resources => {
                let resources = client.list_all_resources().await.context("list resources")?;
                for r in &resources {
                    println!("{}\t{}", r.uri, r.name);
                }
            }
            Command::Call { name, args, json } => {
                let arguments = build_arguments(&args, json.as_deref())?;
                let mut params = CallToolRequestParams::new(name);
                if let Some(obj) = arguments {
                    params = params.with_arguments(obj);
                }
                let result = client.call_tool(params).await.context("call tool")?;
                println!("{}", serde_json::to_string_pretty(&result)?);
                exit_error = result.is_error == Some(true);
            }
            Command::Read { uri } => {
                let result = client
                    .read_resource(ReadResourceRequestParams::new(uri))
                    .await
                    .context("read resource")?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            Command::Workspace { action } => {
                let (name, args) = action
                    .online_call()
                    .expect("workspace use is handled offline before connect");
                let mut params = CallToolRequestParams::new(name);
                if !args.is_empty() {
                    params = params.with_arguments(args);
                }
                let result = client.call_tool(params).await.context("call workspace tool")?;
                println!("{}", serde_json::to_string_pretty(&result)?);
                exit_error = result.is_error == Some(true);
            }
            // Handled offline above, before the transport connect.
            Command::Prime { .. } => unreachable!("prime short-circuits before connect"),
        }
        Ok(())
    }
    .await;

    let _ = client.cancel().await;
    outcome?;
    if exit_error {
        std::process::exit(1);
    }
    Ok(())
}

/// Build the tool-call argument object. `--json` wins if present; otherwise each `--arg k=v`
/// pair becomes one field, with the value parsed as JSON (so `priority=0` is a number and
/// `weekly=true` is a bool) and falling back to a plain string when it is not valid JSON.
fn build_arguments(pairs: &[String], json: Option<&str>) -> Result<Option<Map<String, Value>>> {
    if let Some(raw) = json {
        let value: Value = serde_json::from_str(raw).context("parse --json")?;
        let obj = value
            .as_object()
            .ok_or_else(|| anyhow!("--json must be a JSON object"))?
            .clone();
        return Ok(Some(obj));
    }
    if pairs.is_empty() {
        return Ok(None);
    }
    let mut map = Map::new();
    for pair in pairs {
        let (key, raw) = pair
            .split_once('=')
            .ok_or_else(|| anyhow!("--arg must be in k=v form: {pair}"))?;
        let value =
            serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.to_string()));
        map.insert(key.to_string(), value);
    }
    Ok(Some(map))
}
