//! `gt` — the gt-core client CLI.
//!
//! A thin CLIENT for a gt-core server — it never manages the server's deploy.
//!
//! Commands (alphabetical):
//! - `gt config list|use|show` — manage the per-project `.gt-config/` connection configs.
//! - `gt init` — first-run wizard: log in, pick a workspace + rig, save a per-project config.
//! - `gt mcp` — stdio MCP entrypoint for `.mcp.json`; proxies to the server's `/mcp`.
//! - `gt prime` — report the active workspace/role/rig the shell carries.
//! - `gt register` / `gt unregister` — (de)register those MCP servers in a client config.
//! - `gt tools` — serve gt's own subcommands as MCP tools.
//! - `gt update` — self-update the installed binary to the latest release.
//! - `gt workspace use <id>` — print an `export GT_WORKSPACE=<id>` line to eval.
//!
//! `prime`/`workspace` are offline (env only). `init`/`config`/`mcp` talk to a gt-core server
//! through the `gt-mcp` crate.

use anyhow::Result;
use clap::{Parser, Subcommand};

mod config;
mod config_cmd;
mod init;
mod mcp_cmd;
mod oauth_login;
mod prime;
mod project_config;
mod register;
mod session;
mod tools;
mod update;
mod workspace_cmd;

use init::InitArgs;
use workspace_cmd::WorkspaceAction;

#[derive(Parser)]
#[command(name = "gt", version, about = "gt-core operator CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

// Variants are kept in alphabetical order so `gt --help` lists them alphabetically.
#[derive(Subcommand)]
enum Command {
    /// Manage the per-project named configs under `.gt-config/`.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// First-run wizard: log in, pick a workspace + rig, save a per-project config.
    Init(InitCmd),
    /// Authenticate and save a per-project config — alias of `init`. `gt login` opens the browser
    /// to log in with an OAuth provider; `gt login --token gtpat_…` uses a Personal Access Token.
    /// Same flags as `init`; both end by writing `.gt-config/`.
    Login(InitCmd),
    /// MCP client against the active config: no subcommand = stdio proxy (for `.mcp.json`);
    /// `call`/`list`/`resources`/`resource` drive tools from the shell.
    Mcp {
        #[command(subcommand)]
        action: Option<McpAction>,
    },
    /// Report the active workspace/role/rig. Resolves GT_WORKSPACE > project .gt-config >
    /// user-global default > grace opt-in > abort. Reads the environment + config only.
    Prime {
        /// Emit the context as a JSON object instead of the text report.
        #[arg(long)]
        json: bool,
    },
    /// Register gt (`gt` proxy + `gt-tools`) as MCP servers in a client config.
    Register {
        /// Write to ~/.claude.json instead of the project ./.mcp.json.
        #[arg(long)]
        global: bool,
    },
    /// Serve gt's own subcommands as MCP tools over stdio (for `.mcp.json`).
    Tools,
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
    /// Select a workspace for this shell: `use` prints an `export GT_WORKSPACE=<id>` line to eval.
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },
}

#[derive(clap::Args)]
struct InitCmd {
    /// Server base URL (prompted if omitted).
    #[arg(long, env = "GT_SERVER")]
    server: Option<String>,
    /// Log in with a Personal Access Token (`gtpat_…`) instead of the browser OAuth flow. The
    /// token becomes the access token directly (headless / CI).
    #[arg(long, env = "GT_TOKEN")]
    token: Option<String>,
    /// Workspace id to target (offered as a menu if omitted).
    #[arg(long)]
    workspace: Option<String>,
    /// Rig name or prefix to target (offered as a menu if omitted).
    #[arg(long)]
    rig: Option<String>,
    /// Role this shell speaks as (optional context, e.g. sheriff/deacon).
    #[arg(long)]
    role: Option<String>,
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

#[derive(Subcommand)]
enum McpAction {
    /// Call a tool: `gt mcp call <tool> '<json-args>'` (args: positional JSON, or `-` for stdin).
    Call {
        /// Tool name, e.g. `issues.transition.execute`.
        tool: String,
        /// JSON object of arguments (omit for none; `-` reads stdin).
        args: Option<String>,
    },
    /// List available tools (name + description + input schema).
    List,
    /// Read a resource by URI, e.g. `gt mcp resource 'gt://issues?limit=10'`.
    Resource {
        /// Resource URI.
        uri: String,
    },
    /// List available resources.
    Resources,
    /// Run the stdio proxy (the default when no subcommand is given; for `.mcp.json`).
    Serve,
}

fn main() {
    let cli = Cli::parse();

    // Passive, throttled (~1/day) "newer version available" notice on stderr. Skipped for the
    // stdio MCP servers (`mcp`/`tools` — stdout is their JSON-RPC channel, and they are
    // long-lived) and `update` (which checks already).
    if !matches!(
        cli.cmd,
        Command::Mcp { .. } | Command::Tools | Command::Update { .. }
    ) {
        update::maybe_notify();
    }

    let code = match cli.cmd {
        // Offline commands return their own exit code.
        Command::Prime { json } => prime::run(json),
        Command::Workspace { action } => workspace_cmd::run(&action),
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
            // `init` and its `login` alias share the same flags + flow (both end by saving
            // a config); the only difference is the verb the user types.
            Command::Init(c) | Command::Login(c) => {
                init::run(InitArgs {
                    server: c.server,
                    token: c.token,
                    workspace: c.workspace,
                    rig: c.rig,
                    role: c.role,
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
            Command::Mcp { action } => {
                // Every mcp op runs against the active config with a guaranteed-fresh token.
                let cfg = session::load_fresh().await?;
                match action.unwrap_or(McpAction::Serve) {
                    McpAction::Serve => {
                        gt_mcp::proxy::run(&cfg.server_url, &cfg.access_token, &cfg.workspace).await
                    }
                    McpAction::Call { tool, args } => mcp_cmd::call(&cfg, &tool, args).await,
                    McpAction::List => mcp_cmd::list(&cfg).await,
                    McpAction::Resources => mcp_cmd::resources(&cfg).await,
                    McpAction::Resource { uri } => mcp_cmd::resource(&cfg, &uri).await,
                }
            }
            Command::Tools => tools::run().await,
            Command::Update { check } => update::run(check).await,
            // The offline arms are handled in `main`.
            _ => unreachable!("offline command routed to run_async"),
        }
    });
    to_code(result)
}
