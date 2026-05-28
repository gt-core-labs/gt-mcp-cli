# gt-mcp-cli

Command-line client for the **gt-mcp** server — the MCP surface over the Gas Town
orchestrator (apps/api, Rust). Talks the Model Context Protocol over the streamable-HTTP
transport using the official `rmcp` SDK, so every call is a real MCP handshake.

## Build / install

```sh
cargo build              # target/debug/gt-mcp-cli
cargo install --path .   # installs gt-mcp-cli on PATH (cargo bin)
```

## Usage

```sh
gt-mcp-cli tools                 # list tools (name + description)
gt-mcp-cli tools --full          # full input schemas (JSON)
gt-mcp-cli resources             # list domain snapshot resource URIs
gt-mcp-cli call <name> [--arg k=v ...] [--json '{...}']
gt-mcp-cli read <uri>            # e.g. gt-mcp-cli read gt://agent/sessions
```

- Endpoint: `--url` or `GT_MCP_URL` (default `http://127.0.0.1:8765/mcp`).
- `--arg k=v` values are parsed as JSON (`priority=0` → number, `weekly=true` → bool),
  falling back to a string. `--json` supplies the whole argument object and wins over `--arg`.
- Nonzero exit when the tool reports `isError` or the call fails.

The CLI needs an **HTTP** gt-mcp. Start one:

```sh
GT_MCP_TRANSPORT=http GT_MCP_HTTP_BIND=127.0.0.1:8765 \
  GT_MCP_SCOPE_CONFIG=~/.config/gt-mcp/scope.toml GT_MCP_ACTOR=dev \
  /home/nixos/gastown/apps/api/target/debug/gt-mcp
```

## Letting Claude Code agents use gt-mcp (native MCP)

Claude Code speaks MCP natively: register gt-mcp once and its tools appear as native tools.

### Dev / single host (stdio — isolated in-memory state per session)

Already wired in `~/.claude.json` for this host:

```json
"mcpServers": {
  "gt-mcp": {
    "type": "stdio",
    "command": "/home/nixos/gastown/apps/api/target/debug/gt-mcp",
    "env": {
      "GT_MCP_SCOPE_CONFIG": "/home/nixos/.config/gt-mcp/scope.toml",
      "GT_MCP_ACTOR": "dev",
      "GT_EVENT_LOG": "/tmp/gt.events.jsonl"
    }
  }
}
```

Restart Claude Code to load it. Each session spawns its own gt-mcp → **state is not shared**
across agents. Good for trying the tools; not for a shared orchestrator.

### Shared across agents (HTTP — one server, shared state)

Run **one** gt-mcp HTTP server, point every agent at it:

```json
"mcpServers": {
  "gt-mcp": { "type": "http", "url": "http://<host>:8765/mcp" }
}
```

For Dolt-backed, shared state reachable by the container agents, gt-mcp must run **inside
`gastown-sandbox`** (the host can't reach Dolt `:3307`; the binary is built on the host only
today). Rollout steps when ready:

1. Build gt-mcp inside the container (Rust toolchain + apps/api source) or copy a
   compatible binary in.
2. Run it with `GT_MCP_TRANSPORT=http`, `GT_DOLT_URL=...`, a per-role scope config, and
   optionally `GT_PG_AUDIT_URL=...`.
3. Register the HTTP endpoint in each agent's Claude Code config.
4. Document it in the town `CLAUDE.md` / `gt prime` so agents know it exists.

## Scope (authorization)

gt-mcp is **deny-by-default**. `GT_MCP_SCOPE_CONFIG` points at a per-actor file; `GT_MCP_ACTOR`
selects the connection's actor. Example (`~/.config/gt-mcp/scope.toml`):

```toml
[actors.dev]
allow = ["*"]

[actors.scheduler-bot]
allow = ["scheduling.*", "patrol.tick.execute"]

[actors.watcher]
allow = ["agent.*"]
validate_only = true
```

Grant the narrowest scope a role needs — `*` is full orchestrator control (spawn sessions,
create beads, rotate quota).

## Tools (6 domains, validate/execute pairs)

`agent.{add,remove,transition}` · `scheduling.{enqueue,mark_dispatched,create_bead}` ·
`patrol.{register,heartbeat,tick,close}` · `merge.{submit,start,complete,fail}` ·
`orch.{launch_convoy,complete_member,fail_member}` ·
`quota.{sample,probe,rotate,register}`

Resources: `gt://agent/sessions`, `gt://scheduling/queue`, `gt://patrol/leases`,
`gt://merge/slots`, `gt://orch/convoys`, `gt://quota/accounts`.
