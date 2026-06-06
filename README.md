# gt

The **gt-core client CLI** — a thin client for a gt-core server. It never manages the
server's deploy. Two halves:

- **Offline** — `prime`/`workspace`: report and select the workspace context your shell
  carries (environment only, no network).
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

gt init                   # first-run wizard: log in, pick a workspace + rig, save config
gt login                  # alias of init — opens the browser to log in via an OAuth provider
gt login --token gtpat_…  # …authenticate with a Personal Access Token instead (headless/CI)
gt config list            # per-project named configs under .gt-config/ (active marked *)
gt config use <name>      # switch the active config
gt mcp                    # stdio MCP proxy against the active config (for .mcp.json)
gt mcp list               # list the server's tools
gt mcp call <tool> '<json>'   # call a tool from the shell ('-' reads args from stdin)
gt mcp resource <uri>     # read a resource, e.g. gt mcp resource 'gt://issues?limit=10'
gt tools                  # serve gt's own subcommands as MCP tools (for .mcp.json)
gt register               # write ./.mcp.json registering `gt` + `gt-tools` (--global → ~/.claude.json)
gt unregister             # remove those MCP entries
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

### `init` / `config`

`gt init` (also spelled `gt login` — the same command) is the first-run wizard: it logs in to
a gt-core server (`/auth/login`), lists its workspaces and rigs, lets you pick one of each, and
saves a named config under `.gt-config/` in the project (marked active). The directory holds tokens, so `init` guarantees it is
git-ignored (creating/appending `.gitignore`). Every prompt has a flag
(`--server/--token/--workspace/--rig/--name -y`) for unattended/CI use.

Ways to authenticate, in precedence order:

- **Browser OAuth** (default) — `gt login` discovers the server's login providers
  (`GET /auth/providers`), opens your browser to authorize, and captures the session over a
  one-shot loopback redirect (the token never touches a URL fragment or your shell history). The
  way `claude login` works. 0 providers ⇒ a clear error pointing at `--token`.
- **`--token <gtpat_…>`** (`GT_TOKEN`) — a Personal Access Token used as the access token
  directly (headless / CI). The saved config has an empty `refresh_token` (a PAT has no refresh
  leg), and the token is verified by the next call.

Email+password is no longer a CLI flag — logging in with a password is a browser concern now (the
web app keeps it).

```sh
gt login                                                   # browser OAuth (default)
gt login --token gtpat_… --workspace acme --rig core --name acme -y   # headless / CI
```

`gt config list|use|show` manages the per-project configs — a repo can target several
workspaces/rigs and flip between them.

## Letting Claude Code agents use the orchestrator (MCP)

Two ways to wire the orchestrator's MCP tools into an agent:

For shell scripts (and agents that shell out), drive tools directly:

```sh
gt mcp list                                   # discover tools
gt mcp call issues.transition.execute '{"id":"hq-x.1","target":"working"}'
gt mcp resource 'gt://issues?external_ref=hq-x'
```

These run one authenticated MCP call against the active `.gt-config` (token refreshed
pre-flight) and print JSON. They replace the retired `gt-mcp-cli`.

For an MCP client (Claude Code) wiring the whole tool surface natively:

- **`gt mcp` (per-project, authenticated)** — a stdio↔HTTP proxy that forwards to the
  server's `/mcp`, injecting the active config's bearer token + workspace. It refreshes a
  stale access token from `.gt-config` before connecting (pre-flight `/auth/refresh`), so a
  config saved earlier keeps working. New server tools appear automatically (generic
  passthrough). Point the agent at it:

  ```json
  "mcpServers": { "gt": { "command": "gt", "args": ["mcp"] } }
  ```

- **Direct HTTP (loopback dev, no auth)** — for a local server with auth off:

  ```json
  "mcpServers": { "gt-mcp": { "type": "http", "url": "http://127.0.0.1:8765/mcp" } }
  ```

### Register gt as MCP tools

`gt register` installs two stdio MCP servers into the client config so a model discovers
them automatically — no hand-editing `.mcp.json`:

- `gt` → `gt mcp` — the orchestrator's tools (authenticated, per the active `.gt-config`).
- `gt-tools` → `gt tools` — gt's OWN subcommands as tools (`gt_prime`, `gt_config_*`,
  `gt_workspace_use`, `gt_update_check`, `gt_help`). Each shells out to this
  binary, so behaviour matches the CLI; `tools/list` (with descriptions + server
  instructions) tells the model what exists.

`gt register` writes the project `./.mcp.json`; `gt register --global` writes
`~/.claude.json`. `gt unregister` removes them. Restart the MCP client to pick up changes.
