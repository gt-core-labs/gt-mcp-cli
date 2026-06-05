# gt

The **gt-core operator CLI**. Two halves:

- **Offline** — `prime`/`workspace`/`compose`: report the workspace context your shell
  carries and manage the deploy stack (environment + git + docker, no network).
- **MCP connection** — `init`/`config`/`mcp`: connect a project to a gt-core server, then
  expose its tools to an agent as a stdio MCP proxy. The wire logic lives in the
  [`gt-mcp`](https://crates.io/crates/gt-mcp) crate, kept isolated from this CLI.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/gt-core-labs/gt/main/install.sh | bash
```

Installs the latest `gt` to `~/.local/bin` (override with `GT_INSTALL_DIR`, pin with
`GT_VERSION=vX.Y.Z`). Then keep it current with `gt update`. From source instead:

```sh
cargo install --git https://github.com/gt-core-labs/gt   # or: cargo build
```

## Usage

```sh
gt prime                  # report the active workspace/role/rig (text)
gt prime --json           # …as a JSON object

gt workspace use <id>     # print `export GT_WORKSPACE=<id>` for eval
eval "$(gt workspace use acme)"

gt compose up             # clone gt-app deploy repo + docker compose up -d
gt compose down           # docker compose down — data volumes KEPT
gt compose destroy --yes  # docker compose down --volumes — WIPES data

gt init                   # first-run wizard: log in, pick a workspace + rig, save config
gt config list            # per-project named configs under .gt-config/ (active marked *)
gt config use <name>      # switch the active config
gt mcp                    # stdio MCP proxy against the active config (for .mcp.json)
gt update                 # self-update to the latest release (--check to peek)
```

### `prime`

Every gt command is scoped to a workspace, so `prime` is the guard: it **requires**
`GT_WORKSPACE` and aborts when unset, then reports the resolved workspace plus the role/rig
the shell carries (`GT_ROLE` / `GT_RIG`).

Workspace resolution order: `GT_WORKSPACE` (env) > `default_workspace`
(`~/.config/gt/config.toml`) > the legacy `GT_WORKSPACE_DEFAULT_OPT_IN` grace fallback to
`default` > abort.

```toml
# ~/.config/gt/config.toml
default_workspace = "acme"
```

### `workspace use`

A child process cannot mutate its parent shell, so `use` is offline: it prints an
`export GT_WORKSPACE=<id>` line for the shell to `eval`. The hint goes to stderr so
`eval "$(gt workspace use <id>)"` stays clean.

### `compose`

Clones/updates the [`gt-app`](https://github.com/gt-core-labs/gt-app) deploy repo into
`~/gt-app` (override with `--dir`/`GT_APP_DIR`) and drives `docker compose` against it. `up`
pulls the published `codecsrayo/gt-core-mcp-server` image and starts the dolt+pg+mcp stack
(override the repo with `--repo`/`GT_APP_REPO`).

- **`down` keeps the data.** Tearing the stack down never drops the Dolt/PG/event-log
  volumes — a later `compose up` resumes with the same data.
- **`destroy --yes` wipes the data.** Dropping the volumes is a separate, explicit command
  that refuses to run without `--yes`.

### `init` / `config`

`gt init` is the first-run wizard: it logs in to a gt-core server (`/auth/login`), lists its
workspaces and rigs, lets you pick one of each, and saves a named config under `.gt-config/`
in the project (marked active). The directory holds tokens, so `init` guarantees it is
git-ignored (creating/appending `.gitignore`). Every prompt has a flag
(`--server/--email/--password/--workspace/--rig/--name -y`) for unattended/CI use.

`gt config list|use|show` manages the per-project configs — a repo can target several
workspaces/rigs and flip between them.

## Letting Claude Code agents use the orchestrator (MCP)

Two ways to wire the orchestrator's MCP tools into an agent:

- **`gt mcp` (per-project, authenticated)** — a stdio↔HTTP proxy that forwards to the
  server's `/mcp`, injecting the active config's bearer token + workspace. New server tools
  appear automatically (generic passthrough). Point the agent at it:

  ```json
  "mcpServers": { "gt": { "command": "gt", "args": ["mcp"] } }
  ```

- **Direct HTTP (loopback dev, no auth)** — for a local server with auth off:

  ```json
  "mcpServers": { "gt-mcp": { "type": "http", "url": "http://127.0.0.1:8765/mcp" } }
  ```
