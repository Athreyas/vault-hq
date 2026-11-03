#!/bin/bash
# Build, sign, notarize and checksum a Vault HQ release. Runs ONLY on the maintainer's
# Mac: the Developer ID certificate and the notary credentials live in its keychain and
# never go to CI (SPEC 14). It publishes nothing; at the end it prints the
# `gh release create` command and the updated Homebrew cask for you to run and commit.
#
#   scripts/release.sh --dry-run   every check (version, tag, tree, tools, identity,
#                                  notary profile, entitlements, cask), no build, no signing
#   scripts/release.sh             the release: build app + DMG, sign (hardened runtime),
#                                  notarize, staple, verify, SHA-256, print next steps
#
# Environment:
#   APPLE_SIGNING_IDENTITY   the codesigning identity; default: the one
#                            "Developer ID Application" identity in the keychain
#   NOTARY_PROFILE           notarytool keychain profile (default: vault-hq)
#
# docs/v2/RELEASING.md has the one-time setup and the whole release checklist.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"

dry_run=0
case "${1:-}" in
  --dry-run) dry_run=1 ;;
  "") ;;
  -h | --help) sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  *) echo "usage: scripts/release.sh [--dry-run]" >&2; exit 2 ;;
esac

repo="Athreyas/vault-hq"
app_name="Vault HQ"
profile="${NOTARY_PROFILE:-vault-hq}"
entitlements="src-tauri/entitlements.plist"
cask_template="packaging/homebrew/vault-hq.rb"
out="dist/release"

problems=0
ok() { printf '  ok    %s\n' "$*"; }
bad() { printf '  FAIL  %s\n' "$*" >&2; problems=$((problems + 1)); }
step() { printf '\n== %s\n' "$*"; }
die() { printf 'release: %s\n' "$*" >&2; exit 1; }
# In a real run a failed check stops everything; a dry run reports them all.
gate() { [ "$problems" -eq 0 ] || [ "$dry_run" -eq 1 ] || die "fix the checks above first"; }

step "Machine and tools"
if [ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ]; then
  ok "macOS on Apple Silicon"
else
  bad "releases are built on an Apple Silicon Mac (this is $(uname -s) $(uname -m))"
fi
[ -z "${CI:-}" ] || bad "CI is set: releases are never signed in CI"
for tool in git node npx cargo xcrun codesign security spctl shasum plutil; do
  command -v "$tool" >/dev/null 2>&1 && ok "$tool" || bad "$tool not found"
done
for sub in notarytool stapler; do
  xcrun --find "$sub" >/dev/null 2>&1 && ok "xcrun $sub" || bad "xcrun $sub not found (install Xcode or its command line tools)"
done
command -v gh >/dev/null 2>&1 && ok "gh (for the printed release command)" \
  || printf '  warn  gh not found; you need it to run the printed release command\n'

step "Version"
# One version for the whole app: [workspace.package] in Cargo.toml. tauri.conf.json has
# no version of its own, so Tauri takes this one; package.json must agree.
cargo_version="$(awk '/^\[workspace.package\]/ { s = 1; next } /^\[/ { s = 0 }
  s && /^version *=/ { gsub(/.*= *"|".*/, ""); print; exit }' Cargo.toml)"
npm_version="$(node -p 'require("./package.json").version')"
tauri_version="$(node -p 'require("./src-tauri/tauri.conf.json").version || ""')"
version="$cargo_version"
if [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  ok "Cargo.toml workspace version $version"
else
  bad "Cargo.toml workspace version '$version' is not a release version (X.Y.Z, no -dev)"
fi
[ "$npm_version" = "$version" ] && ok "package.json $npm_version" \
  || bad "package.json version $npm_version differs from Cargo.toml $version"
[ -z "$tauri_version" ] || [ "$tauri_version" = "$version" ] && ok "tauri.conf.json follows Cargo.toml" \
  || bad "src-tauri/tauri.conf.json version $tauri_version differs; remove it so Cargo.toml is the one source"
tag="v$version"
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  bad "tag $tag already exists locally"
elif git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1; then
  bad "tag $tag already exists on origin"
else
  ok "tag $tag is free"
fi

step "Source tree"
branch="$(git rev-parse --abbrev-ref HEAD)"
[ "$branch" = main ] && ok "on main" || bad "on branch '$branch'; release from main"
[ -z "$(git status --porcelain --untracked-files=no)" ] && ok "no uncommitted changes" \
  || bad "uncommitted changes; commit or stash them so the build matches a commit"
git fetch -q origin main 2>/dev/null || true
if git rev-parse -q --verify origin/main >/dev/null; then
  [ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] && ok "HEAD is origin/main" \
    || bad "HEAD differs from origin/main; push or pull first (the release must be a pushed commit)"
fi
[ -x scripts/privacy-check.sh ] && scripts/privacy-check.sh >/dev/null 2>&1 && ok "privacy scan" \
  || bad "scripts/privacy-check.sh fails; run it to see why"

step "Signing identity"
identity="${APPLE_SIGNING_IDENTITY:-}"
if [ -z "$identity" ]; then
  ids="$(security find-identity -v -p codesigning 2>/dev/null \
    | sed -n 's/^ *[0-9][0-9]*) [0-9A-F]\{40\} "\(Developer ID Application: .*\)"$/\1/p')"
  count="$(printf '%s' "$ids" | grep -c . || true)"
  if [ "$count" -eq 1 ]; then
    identity="$ids"
  elif [ "$count" -eq 0 ]; then
    bad "no valid 'Developer ID Application' identity in the keychain (RELEASING.md, one-time setup)"
  else
    bad "several Developer ID Application identities; pick one with APPLE_SIGNING_IDENTITY"
  fi
fi
if [ -n "$identity" ]; then
  if security find-identity -v -p codesigning 2>/dev/null | grep -qF "\"$identity\""; then
    ok "identity: $identity"
  else
    bad "identity '$identity' is not a valid codesigning identity in the keychain"
  fi
fi

step "Notarization"
if xcrun notarytool history --keychain-profile "$profile" >/dev/null 2>&1; then
  ok "keychain profile '$profile' works"
else
  bad "notarytool keychain profile '$profile' missing or rejected: xcrun notarytool store-credentials $profile"
fi

step "Bundle inputs"
plutil -lint -s "$entitlements" && ok "$entitlements" || bad "$entitlements missing or invalid"
grep -q '@@SHA256@@' "$cask_template" 2>/dev/null && ok "$cask_template" \
  || bad "$cask_template missing or has no @@SHA256@@ placeholder"

if [ "$dry_run" -eq 1 ]; then
  step "Dry run"
  if [ "$problems" -eq 0 ]; then
    echo "All checks pass. scripts/release.sh would now build, sign, notarize and staple $tag."
    exit 0
  fi
  echo "$problems check(s) failed; a real run stops here, before building or signing anything." >&2
  exit 1
fi
gate

step "Build ($app_name $version: app + DMG, signed by Tauri with hardened runtime)"
target_dir="$(cargo metadata --format-version 1 --no-deps \
  | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).target_directory))')"
bundle="$target_dir/release/bundle"
rm -rf "$bundle/macos" "$bundle/dmg"
# Tauri signs with APPLE_SIGNING_IDENTITY, the entitlements and hardened runtime from
# tauri.conf.json. Its own notarization is kept off (those variables unset) because
# notarytool below does it with the keychain profile, so no password is in the env.
(
  unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH
  export APPLE_SIGNING_IDENTITY="$identity"
  npx tauri build --bundles app,dmg
)
app="$bundle/macos/$app_name.app"
dmg_built="$(ls "$bundle/dmg/"*.dmg 2>/dev/null | head -1)"
[ -d "$app" ] || die "no app bundle at $app"
[ -n "$dmg_built" ] || die "no DMG in $bundle/dmg"

step "Check the app's signature"
codesign --verify --deep --strict --verbose=2 "$app"
details="$(codesign -dvvv "$app" 2>&1)"
grep -qF "Authority=$identity" <<<"$details" || die "app is not signed by $identity"
grep -q 'flags=.*runtime' <<<"$details" || die "app is not signed with the hardened runtime"
grep -q 'Timestamp=' <<<"$details" || die "app signature has no secure timestamp"
codesign -d --entitlements - --xml "$app" 2>/dev/null | grep -q 'com.apple.security.network.client' \
  || die "app lacks the network.client entitlement"
echo "app signed: $identity, hardened runtime, timestamped"

step "Sign the DMG"
codesign --force --timestamp --sign "$identity" "$dmg_built"
codesign --verify --strict --verbose=2 "$dmg_built"

step "Notarize (this waits for Apple, usually a few minutes)"
result="$(xcrun notarytool submit "$dmg_built" --keychain-profile "$profile" --wait --output-format json)"
echo "$result"
status="$(node -e 'console.log(JSON.parse(process.argv[1]).status)' "$result")"
sub_id="$(node -e 'console.log(JSON.parse(process.argv[1]).id)' "$result")"
if [ "$status" != Accepted ]; then
  xcrun notarytool log "$sub_id" --keychain-profile "$profile" || true
  die "notarization $status (submission $sub_id)"
fi

step "Staple"
xcrun stapler staple "$dmg_built"
# The app's ticket came with the DMG's (same submission); staple the local copy too.
xcrun stapler staple "$app" || echo "note: could not staple the app in $bundle/macos (the DMG is stapled)"

step "Verify"
xcrun stapler validate "$dmg_built"
spctl -a -vvv -t open --context context:primary-signature "$dmg_built"
spctl -a -vvv -t exec "$app"
codesign --verify --deep --strict --verbose=2 "$app"

step "Checksum"
mkdir -p "$out"
# A name without spaces, so the GitHub asset URL is predictable for the cask.
dmg="$out/Vault-HQ_${version}_aarch64.dmg"
cp -f "$dmg_built" "$dmg"
sha="$(shasum -a 256 "$dmg" | awk '{print $1}')"
echo "$sha  $(basename "$dmg")" | tee "$dmg.sha256"

cask="$out/vault-hq.rb"
sed -e "s/@@VERSION@@/$version/g" -e "s/@@SHA256@@/$sha/g" "$cask_template" >"$cask"

step "Next steps (not run; check, then run them yourself)"
cat <<EOF

1. Tag and publish the GitHub release:

   git tag -s $tag -m "Vault HQ $version" && git push origin $tag
   gh release create $tag --repo $repo --verify-tag --title "Vault HQ $version" \\
     --notes "Signed and notarized for macOS 13+ on Apple Silicon. SHA-256: $sha" \\
     "$dmg" "$dmg.sha256"

2. Bump the Homebrew tap (Athreyas/homebrew-tap):

   cp "$cask" <your homebrew-tap clone>/Casks/vault-hq.rb
   cd <your homebrew-tap clone> && git commit -am "vault-hq $version" && git push

   then check it: brew update && brew install --cask athreyas/tap/vault-hq

The cask, as written to $cask:

EOF
cat "$cask"
