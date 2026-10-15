#!/bin/sh
# Install the Orthros sync server as a systemd service.
#
#   curl -fsSL https://freaxmate.github.io/orthros/install-server.sh | sh
#
# Run it again to update. Options (environment variables):
#   ORTHROS_LISTEN=0.0.0.0:8787   listen on the network instead of 127.0.0.1
#   ORTHROS_UNINSTALL=1           remove the service and binary (keeps data)
set -eu

REPO="FreaxMATE/orthros"
BIN="/usr/local/bin/orthros-server"
UNIT="/etc/systemd/system/orthros-server.service"
LISTEN="${ORTHROS_LISTEN:-127.0.0.1:8787}"

say() { printf '\033[1m%s\033[0m\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

if [ "$(id -u)" -eq 0 ]; then
  SUDO=""
elif command -v sudo >/dev/null 2>&1; then
  SUDO="sudo"
else
  die "run as root, or install sudo"
fi

[ "$(uname -s)" = "Linux" ] || die "this script sets up a Linux systemd service. On macOS, download orthros-server-macos-* from https://github.com/$REPO/releases/latest and run it."
command -v systemctl >/dev/null 2>&1 || die "systemd not found. Download the binary from https://github.com/$REPO/releases/latest, or use Docker: docker run -d -p 127.0.0.1:8787:8787 -v orthros:/data ghcr.io/freaxmate/orthros-server"

if [ "${ORTHROS_UNINSTALL:-}" = "1" ]; then
  say "Removing the Orthros server (your data in /var/lib/orthros stays)"
  $SUDO systemctl disable --now orthros-server 2>/dev/null || true
  $SUDO rm -f "$UNIT" "$BIN"
  $SUDO systemctl daemon-reload
  exit 0
fi

case "$(uname -m)" in
  x86_64 | amd64) ARCH="x86_64" ;;
  aarch64 | arm64) ARCH="aarch64" ;;
  *) die "unsupported architecture: $(uname -m)" ;;
esac

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO "$2" "$1"; }
else
  die "needs curl or wget"
fi

say "Downloading orthros-server ($ARCH)"
TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT
fetch "https://github.com/$REPO/releases/latest/download/orthros-server-linux-$ARCH" "$TMP"
$SUDO install -m 755 "$TMP" "$BIN"

say "Setting up the service"
$SUDO tee "$UNIT" >/dev/null <<EOF
[Unit]
Description=Orthros sync server
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=$BIN --data /var/lib/orthros --listen $LISTEN
DynamicUser=yes
StateDirectory=orthros
StateDirectoryMode=0700
UMask=0077
Restart=on-failure
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes

[Install]
WantedBy=multi-user.target
EOF
$SUDO systemctl daemon-reload
$SUDO systemctl enable orthros-server >/dev/null 2>&1
$SUDO systemctl restart orthros-server

# The token is created on first start.
TOKEN=""
for _ in 1 2 3 4 5 6 7 8 9 10; do
  TOKEN="$($SUDO cat /var/lib/orthros/token 2>/dev/null || true)"
  [ -n "$TOKEN" ] && break
  sleep 0.5
done
[ -n "$TOKEN" ] || die "the service didn't start; see: journalctl -u orthros-server"

say "Orthros server is running on $LISTEN"
cat <<EOF

Your sync token:

  $TOKEN

Next steps:
  1. Make it reachable over HTTPS from your devices, for example with Tailscale:
       sudo tailscale serve --bg --https=8444 http://127.0.0.1:8787
  2. On each device:
       orthros --connect https://<this server>:8444 $TOKEN

Update later by running this script again. Show the token again:
  sudo cat /var/lib/orthros/token
EOF
