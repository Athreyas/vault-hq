# hq-mcp

Vault HQ's MCP server (SPEC section 9). Agents start the app binary as
`vault-hq mcp` and speak MCP over stdio: JSON-RPC 2.0, one message per line, protocol
revision 2025-06-18 (2025-03-26 and 2024-11-05 are accepted too). stdout carries protocol
messages only; logs go to stderr.

## What it runs on

The same core the app uses: `hq_core::hq::Hq` opened on the app's folders (settings in
`VAULT_HQ_SUPPORT_DIR`, default `~/Library/Application Support/local.vaulthq.desktop`;
caches such as search vectors in `VAULT_HQ_CACHE_DIR`, default
`~/Library/Caches/local.vaulthq.desktop`). Each tool goes through the module that owns it:

| tool | core |
|---|---|
| read_note, list_notes, create_note (templates) | A `notes` (resolver, `note_create`) |
| append_to_note | `locks::guarded_edit` with the agent's `base_hash` |
| capture, log | A `capture` (the day's capture note) and `days::log` (Agent log) |
| add_task, update_task, list_tasks | B `tasks` (`mutate::add/update/annotate/done/cancel`, `query::run`); `list_tasks.query` is B's `TaskQuery` |
| search, ask | C `search` (BM25 + embeddings) and `search::ask` with the user's local models; Ask over MCP saves no conversation note |
| add_bookmark | F `bookmarks::store::add`, then the app's one capped page fetch |

The app watches its vaults and this process doesn't, so every call starts from a fresh
scan. Tasks are indexed from disk per call; the app's task cache is not touched.

## Which vault it serves

1. `vault-hq mcp --vault <folder>` (repeatable; the first is primary), else
2. `VAULT_HQ_VAULT=<folder>`, else
3. the vaults in the app's settings.

With `--vault` or `VAULT_HQ_VAULT` the app's other settings (blocklist, model server and
models) still apply, through a private copy of settings that is removed on exit; the
app's settings file is never written. `blockedVaultNames` applies in every case. With no
vault, the server still starts and every tool says to set one up in the app.

## Try it

On a throwaway vault, with the MCP Inspector:

```sh
export CARGO_TARGET_DIR=<shared target dir>
npx @modelcontextprotocol/inspector cargo run -q -p hq-mcp --example demo_server
```

Add `-- <folder>` to serve a folder you made, e.g. one from
`node demo/make-demo.mjs /tmp/hq-demo`.

Agent configs, once the app is installed:

```sh
claude mcp add vault-hq -- "/Applications/Vault HQ.app/Contents/MacOS/vault-hq" mcp
```

```json
{ "mcpServers": { "vault-hq": {
    "command": "/Applications/Vault HQ.app/Contents/MacOS/vault-hq", "args": ["mcp"] } } }
```

## Behaviour worth knowing

- `initialize.clientInfo.name` becomes the `author:` of every write (`claude-code` and
  `claude-ai` → `claude`, `cursor-vscode` → `cursor`, `codex-mcp-client` → `codex`, others
  a slug of the name). A client can't write as `human` or `hq`.
- Requests other than `ping` before `initialize` get `-32600`.
- Unknown tool → JSON-RPC `-32602`. Bad arguments, conflicts and anything else a tool
  refuses come back as a tool result with `isError: true`, so the model can read the
  message and correct itself. `structuredContent.error` has `kind` (SPEC 10's error kinds)
  and, on a `Conflict`, `current_hash`.
- Each `tools/call` runs on its own thread; `notifications/cancelled` sets the call's
  cancel flag (`Ctx::is_cancelled`) and its reply is dropped. At EOF, running calls get
  5 s to finish and reply, then are cancelled.
- Nothing is ever deleted or overwritten: `create_note` adds `-2`, `-3`… to a taken
  name, `append_to_note` needs the note's current hash, tasks can be cancelled but not
  deleted, and every write goes through `hq_core::locks`.
