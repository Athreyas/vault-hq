## What and why

<!-- What this changes and why. Link the SPEC section it implements, if any. -->

## How it was tested

<!-- Tests added, and what you ran by hand (with temp dirs or the demo vault, never a real vault). -->

## Checklist

- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
- [ ] `web/src/bindings` regenerated (`cargo test -p hq-core export_bindings`) if types changed
- [ ] `npm run typecheck`, `npm run test`, `npm run build` if `web/` changed
- [ ] New dependencies are MIT/Apache/BSD/ISC/Zlib/MPL-2.0 (`cargo deny check`), none copied from GPL/AGPL code
- [ ] `scripts/privacy-check.sh` passes: no home paths, private IPs, personal emails or names
- [ ] Docs updated where this changes behaviour, a command or a deploy step

<!-- Tauri commands to register, shared-file lines added, anything left unfinished: -->
