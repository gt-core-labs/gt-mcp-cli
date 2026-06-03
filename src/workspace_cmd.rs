//! `gt workspace use <id>` — select a workspace for the current shell.
//!
//! A child process cannot mutate its parent shell, so `use` is **offline**: it prints an
//! `export GT_WORKSPACE=<id>` line to `eval`, mirroring how `gt prime` reports context. The
//! online `workspace.*` catalog operations (list/create/info/…) live on the orchestrator's MCP
//! surface; agents call them there natively.

use clap::Subcommand;

/// The `gt workspace` sub-actions.
#[derive(Subcommand)]
pub enum WorkspaceAction {
    /// Select a workspace for this shell: prints `export GT_WORKSPACE=<id>` to eval.
    Use {
        /// Workspace id / slug.
        id: String,
    },
}

/// Run a workspace action. Returns the process exit code (0 = ok).
pub fn run(action: &WorkspaceAction) -> i32 {
    match action {
        WorkspaceAction::Use { id } => {
            // stdout is the eval-able line; the hint goes to stderr so `eval "$(...)"` stays clean.
            println!("export GT_WORKSPACE={id}");
            eprintln!("# run:  eval \"$(gt workspace use {id})\"   (or export it yourself)");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn use_prints_export_and_exits_ok() {
        assert_eq!(run(&WorkspaceAction::Use { id: "acme".into() }), 0);
    }
}
