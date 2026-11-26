#!/usr/bin/env bash
# Builds and installs monolab-node on a Debian/Ubuntu machine that runs Minecraft servers, or updates
# it. Run as root from the unpacked source folder (the one holding node/ and crates/relay-proto/):
#   sudo RUN_AS=minecraft SERVER_ROOT=/srv/minecraft bash node/deploy/install.sh
# RUN_AS: the user that owns the server folders (made if missing). SERVER_ROOT: the folder holding
# them; the service may write only there and in /var/lib/monolab-node.
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
  echo "Run this as root (sudo)." >&2
  exit 1
fi

SRC="$(cd "$(dirname "$0")/../.." && pwd)"
DATA=/var/lib/monolab-node
RUN_AS="${RUN_AS:-minecraft}"
SERVER_ROOT="${SERVER_ROOT:-/srv/minecraft}"
case "$SERVER_ROOT" in
  /*) ;;
  *) echo "SERVER_ROOT must be an absolute path." >&2; exit 1 ;;
esac

echo "== Packages"
if command -v apt-get >/dev/null 2>&1; then
  apt-get update -q
  DEBIAN_FRONTEND=noninteractive apt-get install -y -q build-essential pkg-config curl ca-certificates tmux tar
fi

if ! command -v cargo >/dev/null 2>&1 && [ ! -x "$HOME/.cargo/bin/cargo" ]; then
  echo "== Installing Rust"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

echo "== Building (takes a few minutes the first time)"
cd "$SRC/node"
cargo build --release --locked

echo "== Installing"
if ! id "$RUN_AS" >/dev/null 2>&1; then
  useradd --system --create-home --home-dir "$SERVER_ROOT" --shell /bin/bash "$RUN_AS"
fi
install -d -m 755 -o "$RUN_AS" -g "$RUN_AS" "$SERVER_ROOT"
install -d -m 700 -o "$RUN_AS" -g "$RUN_AS" "$DATA"
install -m 755 target/release/monolab-node /usr/local/bin/monolab-node
install -m 644 deploy/monolab-node.service /etc/systemd/system/monolab-node.service
install -m 644 deploy/monolab-node-tmux.service /etc/systemd/system/monolab-node-tmux.service
TMUX_BIN="$(command -v tmux)"
install -d -m 755 /etc/systemd/system/monolab-node-tmux.service.d
{
  echo "[Service]"
  echo "User=$RUN_AS"
  echo "Group=$(id -gn "$RUN_AS")"
  echo "ExecStart="
  echo "ExecStart=$TMUX_BIN -S $DATA/tmux.sock new-session -d -s monolab-hold"
} > /etc/systemd/system/monolab-node-tmux.service.d/local.conf

# The user and the server folder for this machine.
install -d -m 755 /etc/systemd/system/monolab-node.service.d
{
  echo "[Service]"
  echo "User=$RUN_AS"
  echo "Group=$(id -gn "$RUN_AS")"
  echo "ReadWritePaths=$SERVER_ROOT"
  case "$SERVER_ROOT" in
    /home/*|/root|/root/*) echo "ProtectHome=false" ;;
  esac
} > /etc/systemd/system/monolab-node.service.d/local.conf

# The config is written once and then edited by hand; only root and the service user read it.
install -d -m 755 /etc/monolab-node
if [ ! -f /etc/monolab-node/config.toml ]; then
  install -m 640 -o root -g "$(id -gn "$RUN_AS")" deploy/config.example.toml /etc/monolab-node/config.toml
fi

DEVICE="$(runuser -u "$RUN_AS" -- /usr/local/bin/monolab-node id --data "$DATA")"

systemctl daemon-reload
systemctl enable monolab-node monolab-node-tmux >/dev/null
# Already running (an update): leave it, the servers run in it.
systemctl start monolab-node-tmux

if ! runuser -u "$RUN_AS" -- /usr/local/bin/monolab-node check --config /etc/monolab-node/config.toml; then
  echo
  echo "Not started: fix /etc/monolab-node/config.toml (grant key, servers, teams), then:"
  echo "  systemctl restart monolab-node"
  echo "Device id: $DEVICE"
  exit 0
fi

systemctl restart monolab-node
sleep 2
systemctl --no-pager --lines=5 status monolab-node || true

echo
echo "Device id: $DEVICE"
echo "Address (for the account server, once it supports nodes): $DATA/addr.json"
echo "Logs: journalctl -u monolab-node -f    Team log: $DATA/team-log/"
