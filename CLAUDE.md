# CLAUDE.md

Vault HQ: a local dashboard over one Obsidian vault (README.md for what it does and how to run it).

## Rules for changing this code

- **One Mac, one vault.** The vault path comes only from `~/.config/vault-automation/vault-path` (or `VAULT_PATH`). Never hardcode a vault name or path, and never read two vaults.
- **Personal code lives only in `server/personal.ts`.** Nothing personal goes in any other file: no device, host, service or vault names. Core code reaches personal features only through `server/extensions.ts`. `scripts/export.sh` is the only way to copy HQ to another machine; it leaves `personal.ts` out and fails on personal terms. If you add a personal term, add it to the export's check too.
- **Task actions go through the `task` CLI** (`server/taskwarrior.ts`), so the Taskwarrior hook syncs the vault. HQ code never rewrites checkbox lines itself; the note editor is the user typing, like editing in Obsidian, and task-sync picks those edits up.
- **All vault writes live in `server/writes.ts`**, so the write surface stays visible in one file. Writes into `09-Planning/` or a capture happen only for text the user typed.
- **Ask stays local.** `server/search.ts` talks only to Ollama (`config.ollama`). Don't add a cloud model path.
- **Writes are guarded.** Every POST needs `X-HQ: 1` and a same-origin `Origin` (`guardWrite`). Inputs are validated with Zod at the boundary.
- **Note edits go only through `saveNote` in `server/writes.ts`**: existing notes only (resolved among scanned notes, never a path joined from the request), refused if the file's SHA-256 changed since the editor read it, written in place under the same `flock` every vault writer uses (never by rename: another writer may hold the lock on that file). Render markdown as React elements (`web/src/components/Rich.tsx`), never with `dangerouslySetInnerHTML`.
- **Settings (`server/settings.ts`)** can change which vault HQ shows, the Taskwarrior program, and the local model server and models, saved to `hq-settings.json` in the state folder. A vault chosen there is HQ-only: the background sync keeps following `~/.config/vault-automation/vault-path`, which only a vault's `install.sh` writes; Settings shows that command, it never runs it. `extension.vaultAllowed` vetoes vaults before anything inside them is read. The Ollama address must stay on this Mac or the home network; keep that check.
- **Design tokens** (colours, type, spacing) live only in `web/src/tokens.css`. No hex values in `app.css` or components. The user's words are set in `--font-human` (IBM Plex Mono), machine output in `--font-machine` (JetBrains Mono).
- **The Mac app (`src-tauri/`) is only a front.** It loads `http://127.0.0.1:4320` and grants the page no Tauri IPC. Anything not served by HQ opens outside the app (`on_navigation`). Rebuild and reinstall with `scripts/app.sh`; Rust comes from Homebrew rustup (`/opt/homebrew/opt/rustup/bin`).
- **Node runs the server TypeScript directly** (type stripping), so use only erasable syntax: no enums, no constructor parameter properties, and `.ts` import extensions.

## Green

`npm run verify` (typecheck and build). For UI changes, also load the page and exercise the changed view in a real browser.
