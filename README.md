# Vault HQ

**One place for your second brain.** Vault HQ is a native Mac app over a folder of plain
markdown that you, your editors and your AI agents all write into. Notes, tasks,
bookmarks and the day's trail live in that folder as files any tool can read; HQ is
where you see them, act on them and ask them questions, with models that run on your
own Mac. Nothing leaves the machine.

> Envisioned and architected by **[Athreyas](https://monk97.me)** · built with **[Claude Code](https://claude.com/claude-code)**

![Home: the day's trail, what needs a decision, and every active project](docs/screenshots/home.png)

## Why

Obsidian is a good way to *store* a folder of markdown and a poor place to *live* in it:
search that doesn't find things by meaning, a layout that doesn't show what needs you,
and a window for looking at files rather than acting on them. Vault HQ keeps the files,
because plain markdown is the right database for knowledge many tools share, and
replaces everything around them.

## What it does

- **Home**: what needs you now, active projects, the day's trail, and widgets you
  arrange yourself (this Mac's load, task burn rate, a JSON status of your choice…).
- **Ask**: a chat with your notes and bookmarks on a local model (Ollama). Answers stream
  in as they are written, cite the notes they used, and every conversation is saved as a
  note.
- **Tasks**: a full task engine (tags, projects, priorities, due/scheduled dates,
  recurrence, dependencies, urgency, grouping, burn-down) whose data is the checkbox lines
  in your notes. Imports from Taskwarrior.
- **Notes**: read and edit any note, with `[[links]]` and backlinks. A save is refused if
  an agent or another app changed the note while you typed; you see what changed and choose.
- **Bookmarks**: imported straight from Chrome, Arc, Dia, Brave, Edge, Firefox and Safari
  bookmark files, only for the browsers you allow. No extension, no login. Old saves
  resurface on Home each day.
- **Quick capture**: ⌥⌘Space or the menu-bar icon, from anywhere. A thought, a task or a
  link, in one line.
- **Your agents, connected**: a built-in MCP server lets Claude Code, Claude Desktop,
  Cursor and Codex search, read and write the vault under the same safety rules as the
  app. Setup writes their configs for you (after showing the diff) and gives each one a
  prompt that teaches it where the brain lives.
- **Several vaults**: home, team, anything; one is primary. Bring an existing Obsidian
  vault and a restructuring assistant tidies it, only where you tick.

| | |
|---|---|
| ![Ask: an answer from a local model, with the passages it read](docs/screenshots/ask.png) | ![Notes: a note rendered in HQ, with links and backlinks](docs/screenshots/notes-reader.png) |
| ![Days: a week of daily notes, metrics and the agents' log](docs/screenshots/days-week.png) | ![Editing: a save refused because an agent changed the note meanwhile](docs/screenshots/editor-conflict.png) |
| ![Ctrl K palette](docs/screenshots/palette.png) | ![Settings: the local model server and models](docs/screenshots/settings-ai.png) |

*Screenshots are from 1.0 and a fictional demo vault; 2.0 keeps the same terminal look.*

## Principles

- **Local only.** No account, no telemetry, no sync service. The model server must be on
  this Mac or your home network; HQ refuses anything else. To use several Macs, keep the
  vault in a folder you already sync (iCloud Drive, Dropbox).
- **Markdown is the source of truth.** Every note, task and bookmark is a plain `.md` file.
  Search vectors and task indexes are caches that can be thrown away.
- **No plugins, no logins.** Agents connect over MCP or by plain file conventions;
  browsers are read from their own files after you say yes.
- **Safe writes.** Every write locks the file, checks it hasn't changed since it was read,
  and writes in place, so the app, your agents and your editors never clobber each other.

## Status

| version | what | branch |
|---|---|---|
| **1.0** | Node server + Tauri window, one vault, Taskwarrior | `main`, tag `v1.0.0` |
| **2.0** | One self-contained Rust app: everything above | `v2`, in development |

2.0 is being built in parallel workstreams against [docs/v2/SPEC.md](docs/v2/SPEC.md).
Signed, notarized builds (DMG and Homebrew) come with the 2.0 release:

```bash
brew install --cask athreyas/tap/vault-hq     # after the 2.0 release
```

## Build from source

macOS 13+ on Apple Silicon, Rust (`brew install rustup && rustup default stable`), Node 25+.

```bash
npm ci
cargo test --workspace            # the core: vaults, notes, tasks, search, bookmarks…
npx tauri dev                     # the app, with live reload of the interface
node demo/make-demo.mjs           # a fictional demo vault in /Users/Shared/hq-demo
```

Point an agent at the vault over MCP:

```bash
claude mcp add vault-hq -- "/Applications/Vault HQ.app/Contents/MacOS/vault-hq" mcp
```

## How it is built

```
crates/hq-core   Rust: vaults, notes, tasks, search and Ask, bookmarks, onboarding, widgets
crates/hq-mcp    Rust: the MCP server (`vault-hq mcp`, stdio)
src-tauri        Rust: the app: windows, menu-bar icon, hotkeys, background jobs
web              TypeScript + React: the interface, talking to the core over Tauri commands
```

One process, no web server. Every pull request runs formatting, lints, tests, a license
check (no GPL/AGPL code), a privacy scan and an app build; see [CONTRIBUTING.md](CONTRIBUTING.md).
Releases are signed and notarized only on the maintainer's Mac
([docs/v2/RELEASING.md](docs/v2/RELEASING.md)).

## Credits

Vault HQ was envisioned and architected by **[Athreyas](https://monk97.me)**, who set its direction, its
principles and every design decision, and was built with **Claude Code** (Anthropic),
working as the engineering team: the spec, the parallel workstreams, the code, the tests
and the reviews. See [AUTHORS.md](AUTHORS.md).

## License

MIT, see [LICENSE](LICENSE).
