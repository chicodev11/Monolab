#!/usr/bin/env bash
# Builds and installs the Monolink account server on Ubuntu, or updates it. Run as root from the unpacked
# source folder (the one holding relay/, crates/relay-proto/ and site/):
#   sudo bash relay/deploy/install.sh
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
  echo "Run this as root (sudo)." >&2
  exit 1
fi

SRC="$(cd "$(dirname "$0")/../.." && pwd)"
DATA=/var/lib/monolab-relay

echo "== Packages"
apt-get update -q
DEBIAN_FRONTEND=noninteractive apt-get install -y -q build-essential pkg-config curl ca-certificates sqlite3 gpg

# Caddy serves https://mnlab.cc (Let's Encrypt) and passes it to the account server on 127.0.0.1:8080.
if ! command -v caddy >/dev/null 2>&1; then
  echo "== Installing Caddy"
  curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/gpg.key | gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt > /etc/apt/sources.list.d/caddy-stable.list
  apt-get update -q
  DEBIAN_FRONTEND=noninteractive apt-get install -y -q caddy
fi

# Building needs more than 1 GB of memory; a swap file keeps the compiler from being killed.
if ! swapon --show | grep -q .; then
  echo "== Adding a 2 GB swap file"
  fallocate -l 2G /swapfile
  chmod 600 /swapfile
  mkswap /swapfile
  swapon /swapfile
  grep -q '^/swapfile ' /etc/fstab || echo '/swapfile none swap sw 0 0' >> /etc/fstab
fi

if ! command -v cargo >/dev/null 2>&1 && [ ! -x "$HOME/.cargo/bin/cargo" ]; then
  echo "== Installing Rust"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
# shellcheck disable=SC1091
. "$HOME/.cargo/env"

echo "== Building (takes a few minutes the first time)"
cd "$SRC/relay"
cargo build --release --locked

# The iroh relay, the same version as the apps' iroh (src-tauri/Cargo.toml, node/Cargo.toml).
IROH_RELAY_VERSION=1.4.1
if ! /usr/local/bin/iroh-relay --version 2>/dev/null | grep -q "$IROH_RELAY_VERSION"; then
  echo "== Building iroh-relay $IROH_RELAY_VERSION"
  cargo install iroh-relay --version "$IROH_RELAY_VERSION" --features server --locked --root /usr/local
fi

echo "== Installing"
id relay >/dev/null 2>&1 || useradd --system --home-dir "$DATA" --shell /usr/sbin/nologin relay
id iroh-relay >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin iroh-relay
install -m 644 deploy/iroh-relay.service /etc/systemd/system/iroh-relay.service
install -d -m 700 -o relay -g relay "$DATA"
install -m 755 target/release/monolab-relay /usr/local/bin/monolab-relay
install -m 644 deploy/monolab-relay.service /etc/systemd/system/monolab-relay.service
# Secrets live outside the source folder; made empty once, filled in by hand.
install -d -m 755 /etc/monolab-relay
if [ ! -f /etc/monolab-relay/env ]; then
  install -m 600 deploy/env.example /etc/monolab-relay/env
  echo "Put the Discord client secret in /etc/monolab-relay/env"
fi
install -m 644 deploy/iroh-relay.toml /etc/monolab-relay/iroh-relay.toml

# Makes the grant key now (first time only) so it can be printed below.
GRANT_PIN="$(runuser -u relay -- /usr/local/bin/monolab-relay --data "$DATA" --grant-pin)"

# Daily backup of the data folder (see backup.sh).
install -m 755 deploy/backup.sh /usr/local/sbin/monolab-relay-backup
install -m 644 deploy/monolab-relay-backup.service /etc/systemd/system/monolab-relay-backup.service
install -m 644 deploy/monolab-relay-backup.timer /etc/systemd/system/monolab-relay-backup.timer

# The website (site/): Caddy serves /var/www/mnlab, a link to the newest copy. The new copy is made
# next to it, then the link is swapped in one rename, so visitors never see a half-copied site.
if [ -d "$SRC/site" ]; then
  echo "== Website"
  install -d -m 755 /var/www/mnlab-releases
  NEW="$(mktemp -d /var/www/mnlab-releases/site-XXXXXX)"
  cp -R "$SRC/site/." "$NEW/"
  chown -R root:root "$NEW"
  chmod -R u=rwX,go=rX "$NEW"
  # The first time /var/www/mnlab may be a plain folder: move it aside so the link can take its place.
  if [ -d /var/www/mnlab ] && [ ! -L /var/www/mnlab ]; then
    mv /var/www/mnlab "/var/www/mnlab-releases/old-$(date +%s)"
  fi
  ln -sfn "$NEW" /var/www/.mnlab-next
  mv -T /var/www/.mnlab-next /var/www/mnlab
  # Keep the newest two copies.
  find /var/www/mnlab-releases -mindepth 1 -maxdepth 1 -type d -printf '%T@ %p\n' | sort -rn | tail -n +3 | cut -d' ' -f2- |
    while read -r OLD; do if [ "$OLD" != "$NEW" ]; then rm -rf -- "$OLD"; fi; done
else
  echo "No site/ folder next to relay/: the website was not updated." >&2
fi

install -d -m 755 /var/log/caddy
chown caddy:caddy /var/log/caddy
install -m 644 deploy/Caddyfile /etc/caddy/Caddyfile

if command -v ufw >/dev/null 2>&1 && ufw status | grep -q "Status: active"; then
  # 80 for Let's Encrypt and the redirect, 443 for mnlab.cc.
  ufw allow 80/tcp >/dev/null
  ufw allow 443/tcp >/dev/null
  # The old v1 relay port is gone (Monolink v1 was removed).
  ufw delete allow 8443/tcp >/dev/null 2>&1 || true
fi

systemctl daemon-reload
systemctl enable monolab-relay iroh-relay monolab-relay-backup.timer caddy >/dev/null
systemctl start monolab-relay-backup.timer

if ! grep -Eq '^MONOLAB_DISCORD_CLIENT_SECRET=.+' /etc/monolab-relay/env; then
  echo
  echo "Not started: MONOLAB_DISCORD_CLIENT_SECRET in /etc/monolab-relay/env is empty."
  echo "Fill it in, then: systemctl restart monolab-relay caddy"
  echo "Grant key (goes into GRANT_KEYS in src-tauri/src/monolink_team.rs): $GRANT_PIN"
  exit 0
fi

systemctl restart monolab-relay
systemctl restart iroh-relay
systemctl restart caddy
sleep 2
systemctl --no-pager --lines=5 status monolab-relay iroh-relay caddy || true

echo
echo "Monolink account server at https://mnlab.cc through Caddy; the website from /var/www/mnlab."
echo "Grant key (number and public key; goes into GRANT_KEYS in src-tauri/src/monolink_team.rs): $GRANT_PIN"
echo "iroh relay at https://mnlab.cc/relay (only devices of an account)."
echo "Logs: journalctl -u monolab-relay -f   journalctl -u iroh-relay -f   journalctl -u caddy -f"
echo "Backups: /var/backups/monolab-relay (daily, 14 days)"
