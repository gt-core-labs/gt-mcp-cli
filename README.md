# gt

The **Gas Town operator CLI** — a small, offline command set for driving a Gas Town
deployment from the shell. It reports the workspace context your shell carries and manages
the deploy stack. It does **not** talk to the orchestrator over MCP: agents speak MCP
natively, so `gt` stays a thin local tool (environment + git + docker).

## Build / install

```sh
cargo build              # target/debug/gt
cargo install --path .   # installs gt on PATH (cargo bin)
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
```

### `prime`

Every Gas Town command is scoped to a workspace, so `prime` is the guard: it **requires**
`GT_WORKSPACE` and aborts when unset, then reports the resolved workspace plus the role/rig
the shell carries (`GT_ROLE` / `GT_RIG`).

Workspace resolution order: `GT_WORKSPACE` (env) > `default_workspace`
(`~/.config/gastown/config.toml`) > the legacy `GT_WORKSPACE_DEFAULT_OPT_IN` grace fallback to
`default` > abort.

```toml
# ~/.config/gastown/config.toml
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

## Letting Claude Code agents use the orchestrator (native MCP)

Claude Code speaks MCP natively — register the gt-mcp server once and its tools appear as
native tools. `gt` itself is not involved in that path; it only stands up the stack
(`gt compose up`) the agents then connect to.

```json
"mcpServers": {
  "gt-mcp": { "type": "http", "url": "http://127.0.0.1:8765/mcp" }
}
```
