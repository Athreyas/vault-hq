# Releasing Vault HQ

Releases are built, signed and notarized **only on the maintainer's Mac**, with
`scripts/release.sh`. CI never holds a certificate, a notary password or a GitHub token
that can publish (SPEC section 14). The script publishes nothing: it ends by printing the
`gh release create` command and the updated Homebrew cask, which you check and run.

What a release is: a DMG with `Vault HQ.app` for Apple Silicon (macOS 13+), signed with
a Developer ID Application certificate under the hardened runtime, notarized by Apple and
stapled; a GitHub Release `vX.Y.Z` carrying the DMG and its SHA-256; and a cask bump in
`Athreyas/homebrew-tap` so `brew install --cask athreyas/tap/vault-hq` gets it.

## One-time setup

1. **Apple Developer Program** membership (needed for Developer ID certificates and
   notarization).

2. **Developer ID Application certificate.** In Xcode: Settings → Accounts → select the
   team → Manage Certificates… → `+` → *Developer ID Application*. Xcode creates the key
   in your login keychain and installs the certificate. Check:

   ```bash
   security find-identity -v -p codesigning     # lists "Developer ID Application: <Name> (<TEAMID>)"
   ```

   `release.sh` uses that identity automatically when there is exactly one; with several,
   set `APPLE_SIGNING_IDENTITY` to the full quoted name. Back up the certificate and key
   (Keychain Access → export as .p12) somewhere safe; it is not recoverable from Apple.

3. **Notary credentials in the keychain.** Create an app-specific password at
   appleid.apple.com (Sign-In and Security → App-Specific Passwords), then:

   ```bash
   xcrun notarytool store-credentials vault-hq \
     --apple-id "<your Apple ID>" --team-id "<TEAMID>" --password "<app-specific password>"
   ```

   The profile name `vault-hq` is what `release.sh` uses (`NOTARY_PROFILE` overrides it).
   An App Store Connect API key works too (`--key`, `--key-id`, `--issuer`).

4. **The Homebrew tap.** Create the public repo `Athreyas/homebrew-tap` (Homebrew maps
   `athreyas/tap` to it) with a `Casks/` folder, and clone it next to this repo:

   ```bash
   gh repo create Athreyas/homebrew-tap --public --description "Homebrew tap for Vault HQ"
   git clone https://github.com/Athreyas/homebrew-tap.git ../homebrew-tap
   mkdir -p ../homebrew-tap/Casks
   ```

5. **Rust and Node** as for development: Homebrew `rustup` (stable toolchain) and Node 25.

Run `scripts/release.sh --dry-run` once on a clean `main` to confirm every piece: it
checks tools, version, tag, tree, the signing identity, the notary profile, the
entitlements and the cask template, and builds or signs nothing.

## Each release

1. **Pick the version** (semver). Set it in `Cargo.toml` (`[workspace.package] version`,
   the one source: `tauri.conf.json` has no version and inherits it) and in
   `package.json`, and refresh the lockfiles:

   ```bash
   cargo update --workspace && npm install --package-lock-only
   ```

2. **Merge to `main` through a PR** so CI checks the exact commit. Wait for it to pass,
   then `git switch main && git pull`.

3. **Dry run**, and fix anything it reports:

   ```bash
   scripts/release.sh --dry-run
   ```

   It also runs `scripts/privacy-check.sh`, which reads your untracked `.privacy-terms`.

4. **Release build:**

   ```bash
   scripts/release.sh
   ```

   It builds `Vault HQ.app` and the DMG with `npx tauri build --bundles app,dmg` (Tauri
   signs the app with your identity, `src-tauri/entitlements.plist` and the hardened
   runtime), verifies the signature, signs the DMG, submits it with
   `xcrun notarytool submit --keychain-profile vault-hq --wait`, staples the ticket,
   verifies with `spctl` and `codesign --verify --deep --strict`, and writes to
   `dist/release/` (ignored by git):
   - `Vault-HQ_X.Y.Z_aarch64.dmg` and `….dmg.sha256`
   - `vault-hq.rb`, the cask with the version and SHA-256 filled in

   If notarization is rejected, the script prints Apple's log for the submission.

5. **Publish** with the commands it printed: create and push the tag, then
   `gh release create vX.Y.Z … dist/release/Vault-HQ_X.Y.Z_aarch64.dmg …sha256`.

6. **Bump the tap:** copy `dist/release/vault-hq.rb` to `Casks/vault-hq.rb` in the
   `homebrew-tap` clone, commit `vault-hq X.Y.Z`, push. Check it on a Mac:

   ```bash
   brew update && brew install --cask athreyas/tap/vault-hq   # or brew upgrade --cask
   brew audit --cask --online athreyas/tap/vault-hq
   ```

7. **Next version:** set `Cargo.toml` and `package.json` to the next `-dev` version on
   `main`.

## Notes

- **Entitlements.** The app is not sandboxed (it reads and writes the vault folders the
  user chooses) and has one entitlement, `com.apple.security.network.client`, to reach the
  local model server. Add an entitlement only with a reason in that file's comment.
- **Stapling.** The DMG is notarized and stapled. The app inside it was covered by the same
  submission, so Gatekeeper accepts it when it is copied out; it checks the ticket online
  the first time. `release.sh` also staples the app left in the build folder.
- **Never** run `release.sh` in CI or put its credentials in GitHub secrets; it refuses to
  run when `CI` is set.
- `src-tauri/tauri.conf.json` holds the signing settings that are not secret
  (`hardenedRuntime`, `entitlements`, the DMG layout); `signingIdentity` stays `null` so
  the identity comes from `APPLE_SIGNING_IDENTITY` at build time.
