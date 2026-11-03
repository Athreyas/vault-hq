#!/bin/bash
# Run Vault HQ as a LaunchAgent: it starts at login, restarts if it crashes, and needs
# no terminal. Usage:
#   scripts/service.sh install    build, write the agent, start it (run again after moving the folder)
#   scripts/service.sh restart    rebuild and restart, after changing the code
#   scripts/service.sh status     is it running, and the last log lines
#   scripts/service.sh uninstall  stop it and remove the agent
set -euo pipefail
LABEL="local.vault-hq"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
LOG="$HOME/Library/Logs/vault-hq.log"
DIR="$(cd "$(dirname "$0")/.." && pwd)"
NODE="$(command -v node || echo /opt/homebrew/bin/node)"
PORT="${HQ_PORT:-4320}"
DOMAIN="gui/$(id -u)"

build() { (cd "$DIR" && npm run --silent build >/dev/null); }

wait_up() {
  for _ in $(seq 1 60); do
    curl -s -m 2 -o /dev/null "http://127.0.0.1:$PORT/" && { echo "Vault HQ is up on http://localhost:$PORT"; return 0; }
    sleep 1
  done
  echo "Vault HQ did not answer on :$PORT within a minute; see $LOG" >&2
  return 1
}

case "${1:-}" in
  install)
    [ -x "$NODE" ] || { echo "node not found" >&2; exit 1; }
    build
    if lsof -ti "tcp:$PORT" -sTCP:LISTEN >/dev/null 2>&1 && ! launchctl print "$DOMAIN/$LABEL" >/dev/null 2>&1; then
      echo "Port $PORT is in use by something else (an HQ started by hand?). Stop it first:" >&2
      echo "  lsof -ti tcp:$PORT -sTCP:LISTEN | xargs kill" >&2
      exit 1
    fi
    mkdir -p "$(dirname "$PLIST")" "$(dirname "$LOG")"
    cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key>
  <array><string>$NODE</string><string>server/index.ts</string></array>
  <key>WorkingDirectory</key><string>$DIR</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HQ_PORT</key><string>$PORT</string>
    <key>PATH</key><string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>ProcessType</key><string>Interactive</string>
  <key>StandardOutPath</key><string>$LOG</string>
  <key>StandardErrorPath</key><string>$LOG</string>
</dict>
</plist>
EOF
    launchctl bootout "$DOMAIN/$LABEL" 2>/dev/null || true
    launchctl bootstrap "$DOMAIN" "$PLIST"
    wait_up
    ;;
  restart)
    build
    launchctl kickstart -k "$DOMAIN/$LABEL"
    wait_up
    ;;
  status)
    if launchctl print "$DOMAIN/$LABEL" >/dev/null 2>&1; then
      launchctl print "$DOMAIN/$LABEL" | grep -E '^\s*(state|pid|last exit code) =' || true
    else
      echo "not installed"
    fi
    [ -f "$LOG" ] && tail -5 "$LOG"
    ;;
  uninstall)
    launchctl bootout "$DOMAIN/$LABEL" 2>/dev/null || true
    rm -f "$PLIST"
    echo "removed; log kept at $LOG"
    ;;
  *)
    sed -n 2,7p "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
