#!/usr/bin/env bash
# Daily copy of the account server's data, run by monolab-relay-backup.timer. Keeps 14 days in
# /var/backups/monolab-relay. The databases are copied with SQLite's own backup (safe while the
# server runs); the grant signing key matters most, since apps carry its public key and a new one
# needs an app update first.
#
# This only guards against mistakes and broken files, not against losing the whole server:
# turn on Vultr's automatic backups for that, or copy this folder somewhere else now and then.
set -euo pipefail

DATA=/var/lib/monolab-relay
OUT=/var/backups/monolab-relay
DAY="$(date -u +%Y-%m-%d)"
DIR="$OUT/$DAY"

install -d -m 700 "$OUT"
rm -rf "$DIR.tmp"
install -d -m 700 "$DIR.tmp"

for db in accounts.db; do
  if [ -f "$DATA/$db" ]; then
    sqlite3 "$DATA/$db" ".backup '$DIR.tmp/$db'"
    sqlite3 "$DIR.tmp/$db" "PRAGMA integrity_check;" | grep -qx ok
  fi
done
# Everything else in the folder (the grant signing key).
find "$DATA" -maxdepth 1 -type f ! -name '*.db' ! -name '*.db-wal' ! -name '*.db-shm' -exec cp -p {} "$DIR.tmp/" \;

rm -rf "$DIR"
mv "$DIR.tmp" "$DIR"
find "$OUT" -mindepth 1 -maxdepth 1 -type d -mtime +14 -exec rm -rf {} +
echo "backed up to $DIR"
