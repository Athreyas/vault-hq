# Rules for every v2 workstream agent

Read `docs/v2/SPEC.md` first; it is the contract. These rules are how to work.

## Where you work
- Only inside your worktree, `$HOME/Coding/Projects/vault-hq-wt/<stream>`, on branch
  `v2-<stream>`. Never edit another worktree, `~/Coding/Projects/vault-hq` (v1, the user's
  running app) or `~/Coding/Projects/vault-hq-public`.
- Environment for every shell command:
  `export PATH=/opt/homebrew/opt/rustup/bin:$PATH CARGO_TARGET_DIR=$HOME/Coding/Projects/vault-hq-wt/target`
  The build folder is shared by all streams to save disk; cargo waits for its lock, so a
  build may pause while another stream compiles. That is normal.
- `node_modules` is a symlink to a shared install. Don't run `npm install`; if you truly
  need a new npm package, say so in your PR instead of adding it.
- The disk is tight. Never leave large files around; never create another `target/`.

## What you may touch
- Only the files your stream owns (SPEC section 15). Shared files (`hq-core/src/lib.rs`,
  `types.rs`, `settings.rs`, `src-tauri/src/main.rs`, `web/src/App.tsx`): add lines only
  in your stream's marked section. If you need a change elsewhere, describe it in the PR.
- Tauri commands: put yours in `src-tauri/src/commands/<area>.rs` as `#[tauri::command]`
  functions; list their names in the PR. Stream D/the integrator registers them.
- Build on the Phase 0 foundation as it is: `error`, `frontmatter`, `locks`, `util`,
  `settings`, `types` (shared section), `vault::{Vault, scan, Note}`. Every write to an
  existing vault file goes through `locks::guarded_edit`/`guarded_write`/`append`.

## Data and privacy (strict)
- Test only with temp dirs and fixtures you create, or the demo vault
  (`node demo/make-demo.mjs /tmp/hq-demo-<stream>`; delete it afterwards).
- **Never read the user's real vaults** (anything under `~/Library/Mobile Documents/`),
  their real browser data, `~/.task`, their agent configs (`~/.cursor`, `~/.codex`,
  `~/Library/Application Support/Claude`), or anything under `~/.config`. Build fixtures
  that imitate those formats instead.
- No personal names, hosts, IPs or paths under `/Users/<name>` in code, tests or docs.
- **License:** the project is MIT. Don't copy code from Karakeep/Daykeep (AGPL) or any GPL
  source; reimplement from the idea. New crates must be MIT/Apache/BSD/ISC/Zlib/MPL-2.0.

## Quality bar (CI runs the same)
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` all pass. `npm run typecheck` and `npm run build` pass if you
  touched `web/`. New logic has tests: parsing, edge cases, the failure paths.
- Errors are `hq_core::Error` with a message a user can act on. No `unwrap()` outside tests.
- Comments say why, not what. Match the existing style.

## Finishing
- Commit in small, clear commits ending with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- `git push -u origin v2-<stream>` and open a PR into `v2`:
  `gh pr create --base v2 --title "v2: <stream>: …" --body "…"`. The body lists what you
  built, the Tauri commands to register, any shared-file lines you added, anything you
  could not finish, and how you tested. End the body with
  `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.
- Don't merge. Your final message: the PR link and a short summary of what's left.
