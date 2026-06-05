//! `gt` — the gt-core operator CLI.
//!
//! - `gt prime` — report the active workspace/role/rig the shell carries.
//! - `gt workspace use <id>` — print an `export GT_WORKSPACE=<id>` line to eval.
//! - `gt compose up|down|destroy` — clone the `gt-app` deploy repo and drive `docker compose`.
//! - `gt init` — first-run wizard: log in, pick a workspace + rig, save a per-project config.
//! - `gt config list|use|show` — manage the per-project `.gt-config/` connection configs.
//! - `gt mcp` — stdio MCP entrypoint for `.mcp.json`; proxies to the server's `/mcp`.
//! - `gt update` — self-update the installed binary to the latest release.
//!
//! `prime`/`workspace`/`compose` are offline (env + git + docker). `init`/`config`/`mcp`
//! talk to a gt-core server through the `gt-mcp` crate.

use anyhow::Result;
use clap::{Parser, Subcommand};

mod compose;
mod config;
mod config_cmd;
mod init;
mod prime;
mod project_config;
mod register;
mod session;
mod tools;
mod update;
mod workspace_cmd;

use compose::ComposeAction;
use init::InitArgs;
use project_config::ConfigStore;
use workspace_cmd::WorkspaceAction;

#[derive(Parser)]
#[command(name = "gt", version, about = "gt-core operator CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Report the active workspace/role/rig. Requires `GT_WORKSPACE` (aborts when unset unless
    /// `GT_WORKSPACE_DEFAULT_OPT_IN` opts into the legacy `default` fallback). Reads the
    /// environment only.
    Prime {
        /// Emit the context as a JSON object instead of the text report.
        #[arg(long)]
        json: bool,
    },
    /// Select a workspace for this shell: `use` prints an `export GT_WORKSPACE=<id>` line to eval.
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },
    /// Manage the gt-app deploy stack: `up` clones the deploy repo + `docker compose up -d`,
    /// `down` tears it down. Drives git + docker.
    Compose {
        #[command(subcommand)]
        action: ComposeAction,
    },
    /// First-run wizard: log in, pick a workspace + rig, save a per-project config.
    Init(InitCmd),
    /// Manage the per-project named configs under `.gt-config/`.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Run the stdio MCP proxy against the active config (for `.mcp.json`).
    Mcp,
    /// Serve gt's own subcommands as MCP tools over stdio (for `.mcp.json`).
    Tools,
    /// Register gt (`gt` proxy + `gt-tools`) as MCP servers in a client config.
    Register {
        /// Write to ~/.claude.json instead of the project ./.mcp.json.
        #[arg(long)]
        global: bool,
    },
    /// Remove gt's MCP server entries from a client config.
    Unregister {
        /// Operate on ~/.claude.json instead of the project ./.mcp.json.
        #[arg(long)]
        global: bool,
    },
    /// Update the installed `gt` binary to the latest GitHub release.
    Update {
        /// Only report whether a newer version exists; do not download.
        #[arg(long)]
        check: bool,
    },
}

#[derive(clap::Args)]
struct InitCmd {
    /// Server base URL (prompted if omitted).
    #[arg(long, env = "GT_SERVER")]
    server: Option<String>,
    /// Login email (prompted if omitted).
    #[arg(long, env = "GT_EMAIL")]
    email: Option<String>,
    /// Login password (prompted, hidden, if omitted).
    #[arg(long, env = "GT_PASSWORD")]
    password: Option<String>,
    /// Workspace id to target (offered as a menu if omitted).
    #[arg(long)]
    workspace: Option<String>,
    /// Rig name or prefix to target (offered as a menu if omitted).
    #[arg(long)]
    rig: Option<String>,
    /// Name to save this config under (defaults to the workspace id).
    #[arg(long)]
    name: Option<String>,
    /// Never prompt; fail if any required value is missing (CI / scripts).
    #[arg(long = "yes", short = 'y')]
    no_interactive: bool,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// List every named config (active marked `*`).
    List,
    /// Set the active config.
    Use {
        /// Config name to activate.
        name: String,
    },
    /// Print the active config (tokens redacted).
    Show,
}

fn main() {
    let cli = Cli::parse();

    // Passive, throttled (~1/day) "newer version available" notice on stderr. Skipped for the
    // stdio MCP servers (`mcp`/`tools` — stdout is their JSON-RPC channel, and they are
    // long-lived) and `update` (which checks already).
    if !matches!(
        cli.cmd,
        Command::Mcp | Command::Tools | Command::Update { .. }
    ) {
        update::maybe_notify();
    }

    let code = match cli.cmd {
        // Offline commands return their own exit code.
        Command::Prime { json } => prime::run(json),
        Command::Workspace { action } => workspace_cmd::run(&action),
        Command::Compose { action } => compose::run(&action),
        Command::Register { global } => to_code(register::run(global, false)),
        Command::Unregister { global } => to_code(register::run(global, true)),
        // Networked / async commands run on a runtime; map Result → exit code.
        cmd => run_async(cmd),
    };
    std::process::exit(code);
}

/// Map a `Result<()>` to a process exit code, printing the error chain on failure.
fn to_code(result: Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    }
}

/// Drive the async subcommands (init/config/mcp/update) and turn the `Result` into a process
/// exit code, printing the error chain on failure.
fn run_async(cmd: Command) -> i32 {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: failed to start the async runtime: {e}");
            return 1;
        }
    };
    let result: Result<()> = rt.block_on(async move {
        match cmd {
            Command::Init(c) => {
                init::run(InitArgs {
                    server: c.server,
                    email: c.email,
                    password: c.password,
                    workspace: c.workspace,
                    rig: c.rig,
                    name: c.name,
                    no_interactive: c.no_interactive,
                })
                .await
            }
            Command::Config { action } => match action {
                ConfigAction::List => config_cmd::list(),
                ConfigAction::Use { name } => config_cmd::use_config(&name),
                ConfigAction::Show => config_cmd::show(),
            },
            Command::Mcp => {
                let store = ConfigStore::discover()?;
                let name = store.active_name()?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "no active config in {} — run `gt init` first",
                        store.dir().display()
                    )
                })?;
                let cfg = store
                    .get(&name)?
                    .ok_or_else(|| anyhow::anyhow!("active config `{name}` is missing"))?;
                // Pre-flight: refresh + persist a stale access token so the proxy never
                // forwards an expired bearer.
                let cfg = session::refresh_if_needed(&store, &name, cfg).await?;
                gt_mcp::proxy::run(&cfg.server_url, &cfg.access_token, &cfg.workspace).await
            }
            Command::Tools => tools::run().await,
            Command::Update { check } => update::run(check).await,
            // The offline arms are handled in `main`.
            _ => unreachable!("offline command routed to run_async"),
        }
    });
    to_code(result)
}
