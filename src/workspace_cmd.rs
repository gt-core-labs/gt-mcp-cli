//! `gt workspace [list|create|info|use]` — operator-facing wrappers over the MCP
//! `workspace.*` tools (`hq-mt-cli.2`).
//!
//! `list`/`create`/`info` are thin clients: they build the tool arguments and the caller
//! dispatches them over the live MCP session, so the server stays the single source of the
//! workspace catalog (create is idempotent + provisions the schema server-side). `use` is
//! **offline** — a child process cannot mutate its parent shell, so it prints an
//! `export GT_WORKSPACE=<id>` line to `eval`, mirroring how `gt prime` reports context.
//! Durable persistence to `~/.config/gastown/config.toml` is `hq-mt-cli.3`.

use clap::Subcommand;
use serde_json::{json, Map, Value};

/// The `gt workspace` sub-actions.
#[derive(Subcommand)]
pub enum WorkspaceAction {
    /// List the workspaces the catalog knows about.
    List,
    /// Create (idempotently provision) a workspace.
    Create {
        /// Workspace id / slug.
        id: String,
        /// Human-readable name (defaults to the id).
        #[arg(long)]
        name: Option<String>,
    },
    /// Show one workspace's details.
    Info {
        /// Workspace id / slug.
        id: String,
    },
    /// Select a workspace for this shell: prints `export GT_WORKSPACE=<id>` to eval.
    Use {
        /// Workspace id / slug.
        id: String,
    },
    /// Suspend an active workspace (reversibly disable).
    Suspend {
        /// Workspace id / slug.
        id: String,
    },
    /// Resume a suspended workspace back to active.
    Resume {
        /// Workspace id / slug.
        id: String,
    },
    /// Archive a workspace (terminal).
    Archive {
        /// Workspace id / slug.
        id: String,
    },
}

impl WorkspaceAction {
    /// Handle the offline action (`use`). Returns `true` when it ran offline (the caller
    /// should exit without opening an MCP session); `false` for the online actions.
    pub fn run_offline(&self) -> bool {
        if let WorkspaceAction::Use { id } = self {
            // stdout is the eval-able line; the hint goes to stderr so `eval "$(...)"` stays clean.
            println!("export GT_WORKSPACE={id}");
            eprintln!("# run:  eval \"$(gt workspace use {id})\"   (or export it yourself)");
            return true;
        }
        false
    }

    /// The MCP tool + arguments for an online action. `None` for the offline `use`.
    pub fn online_call(&self) -> Option<(&'static str, Map<String, Value>)> {
        let mut args = Map::new();
        match self {
            WorkspaceAction::List => Some(("workspace.list", args)),
            WorkspaceAction::Create { id, name } => {
                args.insert("id".into(), json!(id));
                args.insert("name".into(), json!(name.clone().unwrap_or_else(|| id.clone())));
                Some(("workspace.create", args))
            }
            WorkspaceAction::Info { id } => {
                args.insert("id".into(), json!(id));
                Some(("workspace.info", args))
            }
            WorkspaceAction::Suspend { id } => {
                args.insert("id".into(), json!(id));
                Some(("workspace.suspend", args))
            }
            WorkspaceAction::Resume { id } => {
                args.insert("id".into(), json!(id));
                Some(("workspace.resume", args))
            }
            WorkspaceAction::Archive { id } => {
                args.insert("id".into(), json!(id));
                Some(("workspace.archive", args))
            }
            WorkspaceAction::Use { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn use_is_offline_others_online() {
        assert!(WorkspaceAction::Use { id: "acme".into() }.run_offline());
        assert!(!WorkspaceAction::List.run_offline());
        assert!(WorkspaceAction::Use { id: "acme".into() }.online_call().is_none());
    }

    #[test]
    fn list_maps_to_workspace_list_no_args() {
        let (tool, args) = WorkspaceAction::List.online_call().unwrap();
        assert_eq!(tool, "workspace.list");
        assert!(args.is_empty());
    }

    #[test]
    fn create_defaults_name_to_id_and_passes_id() {
        let (tool, args) =
            WorkspaceAction::Create { id: "acme".into(), name: None }.online_call().unwrap();
        assert_eq!(tool, "workspace.create");
        assert_eq!(args["id"], json!("acme"));
        assert_eq!(args["name"], json!("acme"));
    }

    #[test]
    fn create_keeps_explicit_name() {
        let (_t, args) =
            WorkspaceAction::Create { id: "acme".into(), name: Some("Acme Inc".into()) }
                .online_call()
                .unwrap();
        assert_eq!(args["name"], json!("Acme Inc"));
    }

    #[test]
    fn info_passes_id() {
        let (tool, args) = WorkspaceAction::Info { id: "acme".into() }.online_call().unwrap();
        assert_eq!(tool, "workspace.info");
        assert_eq!(args["id"], json!("acme"));
    }

    #[test]
    fn suspend_resume_and_archive_map_to_their_tools() {
        let (t1, a1) = WorkspaceAction::Suspend { id: "acme".into() }.online_call().unwrap();
        assert_eq!(t1, "workspace.suspend");
        assert_eq!(a1["id"], json!("acme"));
        let (tr, ar) = WorkspaceAction::Resume { id: "acme".into() }.online_call().unwrap();
        assert_eq!(tr, "workspace.resume");
        assert_eq!(ar["id"], json!("acme"));
        let (t2, a2) = WorkspaceAction::Archive { id: "acme".into() }.online_call().unwrap();
        assert_eq!(t2, "workspace.archive");
        assert_eq!(a2["id"], json!("acme"));
        // all online (not the offline `use`)
        assert!(!WorkspaceAction::Suspend { id: "acme".into() }.run_offline());
        assert!(!WorkspaceAction::Resume { id: "acme".into() }.run_offline());
        assert!(!WorkspaceAction::Archive { id: "acme".into() }.run_offline());
    }
}
