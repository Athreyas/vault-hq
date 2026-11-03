# hq-mcp

Vault HQ's MCP server (SPEC section 9). Agents start the app binary as
`vault-hq mcp` and speak MCP over stdio: JSON-RPC 2.0, one message per line, protocol
revision 2025-06-18 (2025-03-26 and 2024-11-05 are accepted too). stdout carries protocol
messages only; logs go to stderr.

## Which vault it serves

1. `vault-hq mcp --vault <folder>` (repeatable; the first is primary), else
2. `VAULT_HQ_VAULT=<folder>`, else
3. the app's settings (`VAULT_HQ_SUPPORT_DIR`, default
   `~/Library/Application Support/local.vaulthq.desktop/settings.json`).

The settings' `blockedVaultNames` apply in every case. With no vault, the server still
starts and every tool says to set one up in the app.

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
- Nothing is ever deleted or overwritten: `create_note` refuses an existing file,
  `append_to_note` needs the note's current hash, and both go through `hq_core::locks`.
