#!/bin/bash
# Privacy scan: fails if the repo carries anything that points at one person's machine
# or network. The repo is public, so this runs in CI on every push and PR (SPEC 14).
#
# Fails on:
#   - home paths `/Users/<name>` (`/Users/Shared` is fine: the demo lives there)
#   - private IPv4 literals (10/8, 172.16/12, 192.168/16, 100.64/10)
#   - email addresses, except noreply ones and the RFC 2606 example domains
#   - any term in `.privacy-terms` (one per line, `#` comments, case-insensitive). That
#     file is untracked and lives only on the maintainer's Mac; CI has the generic checks.
#
# A line that has to hold such a value on purpose (a test of the "Ollama must stay on
# this network" rule, say) carries the marker `privacy-check: allow` in a comment.
#
# Usage: scripts/privacy-check.sh [--terms FILE]     (or PRIVACY_TERMS=FILE)
set -euo pipefail
cd "$(dirname "$0")/.."

terms_file="${PRIVACY_TERMS:-.privacy-terms}"
if [ "${1:-}" = "--terms" ]; then
  terms_file="${2:?--terms needs a file}"
  [ -f "$terms_file" ] || { echo "privacy-check: no such terms file: $terms_file" >&2; exit 2; }
fi

# Every file git would publish: tracked plus untracked-but-not-ignored, minus the
# folders and file kinds that are generated, vendored or binary.
files=()
while IFS= read -r -d '' f; do
  [ -f "$f" ] && [ ! -L "$f" ] || continue
  case "$f" in
    node_modules/* | */node_modules/* | target/* | */target/* | web/dist/* | dist/*) continue ;;
    package-lock.json | */package-lock.json | Cargo.lock | */Cargo.lock) continue ;;
    .privacy-terms | "$terms_file") continue ;;
    *.png | *.jpg | *.jpeg | *.gif | *.webp | *.ico | *.icns | *.svg | *.pdf) continue ;;
    *.woff | *.woff2 | *.ttf | *.otf | *.dmg | *.zip | *.gz) continue ;;
  esac
  files+=("$f")
done < <(git ls-files -z --cached --others --exclude-standard)

if [ "${#files[@]}" -eq 0 ]; then
  echo "privacy-check: no files to scan" >&2
  exit 1
fi

failed=0
report() { # $1 = what, stdin = grep -n output
  local hits
  hits="$(grep -v 'privacy-check: allow' || true)"
  if [ -n "$hits" ]; then
    echo "privacy-check: $1:"
    printf '%s\n' "$hits" | sed 's/^/  /'
    failed=1
  fi
}

scan() { # $1 = extended regex; prints file:line:text, binary files skipped
  grep -nHIE -- "$1" "${files[@]}" || true
}

# Keeps the lines that still match $BAD once every $ALLOWED match is removed, so a line
# with one allowed value and one personal value is still caught.
still() { # $1 = allowed ERE, $2 = bad ERE
  ALLOWED="$1" BAD="$2" awk '{ t = $0; sub(/^[^:]*:[0-9]+:/, "", t)
    gsub(ENVIRON["ALLOWED"], "", t); if (t ~ ENVIRON["BAD"]) print }'
}

home='/Users/[A-Za-z0-9._-]+'
report "home folder paths (use ~ or a temp dir)" < <(
  scan "$home" | still '/Users/Shared([^A-Za-z0-9._-]|$)' "$home")

octet='(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9]?[0-9])'
private="(10\\.$octet|172\\.(1[6-9]|2[0-9]|3[01])|192\\.168|100\\.(6[4-9]|[7-9][0-9]|1[01][0-9]|12[0-7]))\\.$octet\\.$octet"
# Bounded so version strings like 1.10.2.3 or 210.10.0.1 don't count.
report "private IPv4 addresses" < <(
  scan "(^|[^0-9.])$private([^0-9.]|\\.?\$|\\.[^0-9])")

email='[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}'
# noreply senders (commit trailers), RFC 2606 example domains, git remotes, and
# retina image names (icon@2x.png) are not anybody's address.
ok_email='[A-Za-z0-9._%+-]*([Nn][Oo]-?[Rr][Ee][Pp][Ll][Yy])[A-Za-z0-9._%+-]*@[A-Za-z0-9.-]+|[A-Za-z0-9._%+-]+@(example\.(com|org|net)|localhost)|git@github\.com|[A-Za-z0-9._-]+@[0-9]+x\.(png|jpg|jpeg|webp)'
report "email addresses (only noreply or example.com ones)" < <(
  scan "$email" | still "$ok_email" "$email")

if [ -f "$terms_file" ]; then
  terms=()
  while IFS= read -r t || [ -n "$t" ]; do
    t="${t%%#*}"
    t="$(printf '%s' "$t" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
    [ -n "$t" ] && terms+=(-e "$t")
  done <"$terms_file"
  if [ "${#terms[@]}" -gt 0 ]; then
    report "terms from $terms_file" < <(grep -nHIiF "${terms[@]}" -- "${files[@]}" || true)
  fi
fi

if [ "$failed" -ne 0 ]; then
  echo "privacy-check: FAILED (${#files[@]} files scanned)" >&2
  exit 1
fi
echo "privacy-check: ok (${#files[@]} files scanned)"
