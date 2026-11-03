# Contributing to Vault HQ

Vault HQ is MIT-licensed. `docs/v2/SPEC.md` is the contract for 2.0: read the sections
for the area you change before you change it.

## Setup

macOS on Apple Silicon, Rust stable (Homebrew `rustup`), Node 25 (`npm`, never another
package manager). `npm ci` installs the UI and the Tauri CLI.

## The checks

CI (`.github/workflows/ci.yml`) runs these on every pull request and every push to `v2`
and `main`; all must pass before a merge. Run them locally first:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p hq-core export_bindings && git status --short web/src/bindings   # must print nothing
npm run typecheck && npm run test && npm run build
npm audit --omit=dev --audit-level=high
cargo deny check                # brew install cargo-deny
npx tauri build --no-bundle --debug
scripts/privacy-check.sh
```

What they guard:

- **Rust:** formatted, no clippy warnings, tests pass. New logic comes with tests,
  including the failure paths. No `unwrap()` outside tests.
- **Bindings:** `web/src/bindings/` is generated from `crates/hq-core/src/types.rs` by
  ts-rs. Never edit it by hand; regenerate and commit it with the type change.
- **Web:** TypeScript strict typecheck, vitest, a production build.
- **Licences and advisories:** `cargo deny check` against `deny.toml`. Dependencies must
  be MIT, Apache-2.0, BSD, ISC, Zlib, Unicode, MPL-2.0, CC0, 0BSD or BSL-1.0. GPL, AGPL
  and LGPL are refused, so don't copy code from such projects either: reimplement the
  idea. Known RustSec advisories fail the build; `npm audit` fails on high or critical
  issues in runtime dependencies.
- **The app compiles:** `tauri build --no-bundle`.
- **Privacy:** the repo is public. `scripts/privacy-check.sh` fails on home paths
  (`/Users/<name>`), private IPv4 addresses, email addresses other than noreply and
  `example.com` ones, and any term in your own untracked `.privacy-terms` file (one per
  line). Tests use temp dirs and the demo vault (`node demo/make-demo.mjs <dir>`), never
  a real vault. A line that must hold such a value on purpose (a test of the
  local-network rule) carries the comment `privacy-check: allow`.

## Pull requests

Branch from `v2` (or `main` after 2.0), keep commits small and clear, and open the PR
against the branch you started from. The template asks what changed, how you tested it
and which checks you ran. Releases are not part of a PR: they are signed on the
maintainer's Mac (`docs/v2/RELEASING.md`).
