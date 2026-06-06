//! Per-project config under `.gt-config/` (hq-gt-cli).
//!
//! Layout, rooted at the working project's repo root (the nearest ancestor with a
//! `.git`):
//!
//! ```text
//! .gt-config/
//!   config.toml      # { active = "<name>" } — which named config is current
//!   <name>.toml      # one [`ProjectConfig`] per named config (server + ws + rig + tokens)
//! ```
//!
//! Multiple named configs per project is the whole point: a repo may target several
//! workspaces/rigs, and `gt config use <name>` flips the active pointer. The directory
//! holds tokens, so it must be git-ignored — [`ensure_gitignored`] guarantees that,
//! creating `.gitignore` when absent and appending the entry when missing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The config directory name, also the `.gitignore` entry.
pub const CONFIG_DIR: &str = ".gt-config";
/// The active-pointer file inside [`CONFIG_DIR`].
const ACTIVE_FILE: &str = "config.toml";

/// One named config: where the server is, which tenant + rig this project targets,
/// and the token pair `gt init` obtained at login. Tokens live here (not the env)
/// because the directory is git-ignored; `gt mcp` reads them to authenticate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Base URL of the gt-mcp-server, e.g. `https://gt.codecsrayo.com`.
    pub server_url: String,
    /// The workspace id (the `X-Workspace` value / token claim) this config targets.
    pub workspace: String,
    /// The rig prefix new beads land under (issues.create routing).
    pub rig: String,
    /// The role this shell speaks as (e.g. sheriff/deacon). Optional context surfaced by
    /// `gt prime`; defaulted for back-compat with configs written before it existed.
    #[serde(default)]
    pub role: Option<String>,
    /// The RS256 access JWT from `/auth/login`. Short-lived; refreshed on 401.
    pub access_token: String,
    /// The opaque refresh token, exchanged at `/auth/refresh` when the access expires.
    pub refresh_token: String,
}

/// The `config.toml` active-pointer.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Active {
    /// Name of the currently selected config (a `<name>.toml` stem).
    active: Option<String>,
}

/// Resolved paths for a project's `.gt-config/`. Built once from the repo root so the
/// rest of the module never re-walks the tree.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    /// Repo root the `.gt-config/` lives at.
    root: PathBuf,
}

impl ConfigStore {
    /// Build a store rooted at the working project's repo root — the nearest ancestor
    /// of `cwd` holding a `.git`, or `cwd` itself when none is found (a non-git dir is
    /// still a valid project for config purposes).
    pub fn discover() -> Result<Self> {
        let cwd = std::env::current_dir().context("resolve current directory")?;
        Ok(Self::at(find_repo_root(&cwd)))
    }

    /// Build a store rooted at an explicit directory (used by tests).
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    /// `<root>/.gt-config`.
    pub fn dir(&self) -> PathBuf {
        self.root.join(CONFIG_DIR)
    }

    fn named_path(&self, name: &str) -> PathBuf {
        self.dir().join(format!("{name}.toml"))
    }

    fn active_path(&self) -> PathBuf {
        self.dir().join(ACTIVE_FILE)
    }

    /// Names of every saved config (the `<name>.toml` stems), sorted, excluding the
    /// active-pointer file itself. Empty when `.gt-config/` does not exist yet.
    pub fn list(&self) -> Result<Vec<String>> {
        let dir = self.dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let Some(stem) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if stem == ACTIVE_FILE {
                continue;
            }
            if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                names.push(name.to_string());
            }
        }
        names.sort();
        Ok(names)
    }

    /// Read one named config; `None` when no such `<name>.toml` exists.
    pub fn get(&self, name: &str) -> Result<Option<ProjectConfig>> {
        let path = self.named_path(name);
        if !path.exists() {
            return Ok(None);
        }
        let raw =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let cfg = toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
        Ok(Some(cfg))
    }

    /// The name of the active config, if the pointer is set and the file exists.
    pub fn active_name(&self) -> Result<Option<String>> {
        let path = self.active_path();
        if !path.exists() {
            return Ok(None);
        }
        let raw =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let active: Active =
            toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
        Ok(active.active)
    }

    /// The active config itself, resolving the pointer through [`get`](Self::get). Kept as a
    /// convenience accessor (used by tests); callers that also need the name use
    /// [`active_name`](Self::active_name) + [`get`](Self::get).
    #[allow(dead_code)]
    pub fn active(&self) -> Result<Option<ProjectConfig>> {
        match self.active_name()? {
            Some(name) => self.get(&name),
            None => Ok(None),
        }
    }

    /// Write a named config (creating `.gt-config/` if needed) and, when `make_active`,
    /// point the active file at it. Always ensures the directory is git-ignored first,
    /// so a token never lands in a tracked file.
    pub fn save(&self, name: &str, cfg: &ProjectConfig, make_active: bool) -> Result<()> {
        ensure_gitignored(&self.root)?;
        let dir = self.dir();
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let body = toml::to_string_pretty(cfg).context("serialize config")?;
        let path = self.named_path(name);
        std::fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
        if make_active {
            self.set_active(name)?;
        }
        Ok(())
    }

    /// Point the active file at `name`; errors when no such config exists, so the
    /// pointer can never dangle.
    pub fn set_active(&self, name: &str) -> Result<()> {
        if self.get(name)?.is_none() {
            anyhow::bail!("no config named `{name}` in {}", self.dir().display());
        }
        let body = toml::to_string(&Active {
            active: Some(name.to_string()),
        })
        .context("serialize active pointer")?;
        let path = self.active_path();
        std::fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }
}

/// Normalize a server base URL: drop a trailing `/` and a trailing `/mcp` segment, so a user
/// who pastes the MCP endpoint (`https://host/mcp`) still gets the REST base (`https://host`).
/// The proxy/invoke append `/mcp` themselves, so a stored `/mcp` would otherwise double it, and
/// REST (`/auth/login`, `/api/v1/*`) would hit the MCP transport (→ HTTP 406).
pub fn normalize_server_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    let base = trimmed.strip_suffix("/mcp").unwrap_or(trimmed);
    base.trim_end_matches('/').to_string()
}

/// Walk up from `start` to the nearest ancestor containing a `.git` entry; fall back
/// to `start` when the tree has none.
fn find_repo_root(start: &Path) -> PathBuf {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        cur = dir.parent();
    }
    start.to_path_buf()
}

/// Guarantee `<root>/.gitignore` ignores `.gt-config/` (hq-gt-cli requirement):
/// - no `.gitignore` ⇒ create it with the single entry;
/// - present without the entry ⇒ append it (preserving existing content);
/// - already present ⇒ no-op.
///
/// Returns `true` when the file was created or modified, `false` when it was already
/// correct — handy for a "wrote .gitignore" log line, and asserted by the tests.
pub fn ensure_gitignored(root: &Path) -> Result<bool> {
    let path = root.join(".gitignore");
    let entry = format!("{CONFIG_DIR}/");
    if !path.exists() {
        std::fs::write(&path, format!("{entry}\n"))
            .with_context(|| format!("create {}", path.display()))?;
        return Ok(true);
    }
    let existing =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    // Match either `.gt-config` or `.gt-config/` as an exact, trimmed line so a
    // substring (e.g. a comment) never counts as already-ignored.
    let already = existing.lines().any(|l| {
        let t = l.trim();
        t == entry || t == CONFIG_DIR
    });
    if already {
        return Ok(false);
    }
    let mut next = existing;
    if !next.ends_with('\n') && !next.is_empty() {
        next.push('\n');
    }
    next.push_str(&entry);
    next.push('\n');
    std::fs::write(&path, next).with_context(|| format!("write {}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ws: &str) -> ProjectConfig {
        ProjectConfig {
            server_url: "http://127.0.0.1:8765".into(),
            workspace: ws.into(),
            rig: "core".into(),
            role: None,
            access_token: "access".into(),
            refresh_token: "refresh".into(),
        }
    }

    #[test]
    fn ensure_gitignored_creates_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let changed = ensure_gitignored(tmp.path()).unwrap();
        assert!(changed, "fresh dir → created");
        let body = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert_eq!(body, ".gt-config/\n");
    }

    #[test]
    fn ensure_gitignored_appends_when_missing_entry() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "target/\n").unwrap();
        let changed = ensure_gitignored(tmp.path()).unwrap();
        assert!(changed);
        let body = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert_eq!(body, "target/\n.gt-config/\n");
    }

    #[test]
    fn ensure_gitignored_noop_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "a/\n.gt-config/\nb/\n").unwrap();
        let changed = ensure_gitignored(tmp.path()).unwrap();
        assert!(!changed, "already ignored → no change");
        let body = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert_eq!(body, "a/\n.gt-config/\nb/\n", "content untouched");
    }

    #[test]
    fn ensure_gitignored_accepts_unslashed_entry() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), ".gt-config\n").unwrap();
        assert!(!ensure_gitignored(tmp.path()).unwrap(), "bare form counts");
    }

    #[test]
    fn save_get_roundtrip_and_active() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ConfigStore::at(tmp.path().to_path_buf());
        store.save("acme", &sample("acme"), true).unwrap();
        store.save("globex", &sample("globex"), false).unwrap();

        assert_eq!(store.get("acme").unwrap().unwrap(), sample("acme"));
        assert_eq!(store.list().unwrap(), vec!["acme", "globex"]);
        assert_eq!(store.active_name().unwrap().as_deref(), Some("acme"));
        assert_eq!(store.active().unwrap().unwrap().workspace, "acme");

        store.set_active("globex").unwrap();
        assert_eq!(store.active().unwrap().unwrap().workspace, "globex");
    }

    #[test]
    fn save_ignores_dir_and_get_missing_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ConfigStore::at(tmp.path().to_path_buf());
        store.save("acme", &sample("acme"), true).unwrap();
        // save() ensured the .gitignore entry exists.
        let gi = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert!(gi.contains(".gt-config/"));
        assert!(store.get("nope").unwrap().is_none());
    }

    #[test]
    fn set_active_rejects_unknown() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ConfigStore::at(tmp.path().to_path_buf());
        assert!(store.set_active("ghost").is_err());
    }

    #[test]
    fn list_empty_when_no_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ConfigStore::at(tmp.path().to_path_buf());
        assert!(store.list().unwrap().is_empty());
        assert!(store.active().unwrap().is_none());
    }

    #[test]
    fn normalize_strips_mcp_and_slash() {
        assert_eq!(normalize_server_url("https://h/mcp"), "https://h");
        assert_eq!(normalize_server_url("https://h/mcp/"), "https://h");
        assert_eq!(normalize_server_url("https://h/"), "https://h");
        assert_eq!(
            normalize_server_url("http://127.0.0.1:8765"),
            "http://127.0.0.1:8765"
        );
        assert_eq!(normalize_server_url("  https://h/mcp  "), "https://h");
    }

    #[test]
    fn find_repo_root_walks_up() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let nested = tmp.path().join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_repo_root(&nested), tmp.path());
    }
}
