# Monolink account server

The account server for Monolink (sharing servers between Monolab apps): Discord sign-in, teams,
roles and signed role grants, plus update notices and anonymous usage counts for every running app
(`/v2/app`) and a small admin page (`/admin`). It runs on loopback and Caddy serves it at
https://mnlab.cc, together with the static website from `site/`.

## What Caddy sends where (`deploy/Caddyfile`)

- `/v2/*`, `/auth/*`, `/invite/*`, `/admin`, `/admin/*` → the account server on `127.0.0.1:8080`.
- `/relay`, `/ping`, `/generate_204` → `iroh-relay` on `127.0.0.1:3340`.
- `/v2/relay-access` and `/v2/admin/*` → 404 from outside (only callers on this machine use them).
- Everything else (`/`, `/privacy`, `/changelog`, …) → the website in `/var/www/mnlab`
  (`{path}`, `{path}.html`, `{path}/index.html`; a missing page shows `/404.html` when the site has one).

## App connection `/v2/app`

A WebSocket every app opens, signed in or not (design: `docs/updates-and-stats.md` §3). The app sends
`{"type":"hello","app":"1.0.2","addon":41,"fork":0,"os":"windows-x86_64","install":"<32 lowercase hex>"|null}`
first, then `{"type":"ping"}` every 60 seconds; the server answers `pong` and sends
`{"type":"updates","addon":N,"fork":M}` right after the hello and whenever the release list changes.

- The server reads `https://github.com/chicodev11/Monolab/releases/download/updates/updates.json` every
  5 minutes and takes only `addon.version` and `fork.build` (apps check the signature themselves).
- `install` set (usage counts on): the install's versions and OS go in `installs`, and one row per UTC
  day in `install_days`; daily rows and installs older than 400 days are dropped once a day.
  `install` null: only an in-memory count of connections.
- IP addresses are never stored: they're held in memory only for the per-address limits (20 new
  connections a minute, 16 open at once; 50,000 open in all).

### Telling apps about a release right away

After a new `updates.json` is up, on the server:

```
monolab-relay --notify-updates
```

It calls `POST /v2/admin/notify-updates` on the running server at `--api` (default `127.0.0.1:8080`),
which then reads the release list at once. The endpoint answers only calls from this machine that
didn't come through Caddy, and Caddy answers 404 for `/v2/admin/*`.

## Admin page `/admin`

https://mnlab.cc/admin shows apps connected now, users today / 7 days / 30 days, accounts, signed-in
devices, app / add-on / fork versions and OS, and the release list's add-on and fork numbers.

- Sign-in is Discord in the browser through the same `/auth/callback/discord` the apps use (told apart by
  the OAuth state), so the Discord app's redirect URL stays as it is. The session is a 12-hour
  `HttpOnly; Secure; SameSite=Lax` cookie; only its hash is stored.
- Who may open it: `MONOLAB_ADMIN_USERS` in `/etc/monolab-relay/env`, Monolab account ids or permanent
  8-digit numbers, comma-separated (handles aren't accepted: they can change hands). Anyone else gets
  403. Add the line to an existing env file by hand, then `systemctl restart monolab-relay`.

## Run locally

```
cd relay
cargo test
cargo run -- --api 127.0.0.1:8080 --public-url http://127.0.0.1:8080 --fake-sign-in --data ./.data
```

`--fake-sign-in` (a pretend sign-in at `/auth/fake` instead of Discord) only works with a localhost
`--public-url`. For real Discord sign-in set `MONOLAB_DISCORD_CLIENT_SECRET` instead.

## Install or update on a server (Ubuntu)

From the repository root in PowerShell:

```
tar --exclude=target -czf $env:TEMP\relay-src.tgz relay crates/relay-proto site
scp $env:TEMP\relay-src.tgz root@<server>:/root/
ssh root@<server>
```

On the server:

```
rm -rf /root/monolab-src && mkdir -p /root/monolab-src
tar -xzf /root/relay-src.tgz -C /root/monolab-src
bash /root/monolab-src/relay/deploy/install.sh
```

`install.sh` also copies `site/` to a new folder under `/var/www/mnlab-releases` and swaps the
`/var/www/mnlab` link to it in one step (the newest two copies are kept).

Put the Discord client secret (and `MONOLAB_ADMIN_USERS`) in `/etc/monolab-relay/env` (see `deploy/env.example`). Keep
`/var/lib/monolab-relay/grant-key.bin` when moving servers: apps carry its public key
(`monolab-relay --grant-pin` prints it). Open 80/tcp and 443/tcp for Caddy.

`install.sh` also builds `iroh-relay` (the version the apps use) and runs it on `127.0.0.1:3340`; Caddy serves
it at `https://mnlab.cc/relay`. It carries teammates' connections when there's no direct path, and only
for devices of an account (it asks the account server at `/v2/relay-access`, which Caddy hides from outside).
The apps and nodes use it from 1.0.0 on; `MONOLAB_IROH_RELAY=n0` puts one back on n0's public relays.

- Status: `systemctl status iroh-relay`
- Logs: `journalctl -u iroh-relay -f`

- Status: `systemctl status monolab-relay`
- Logs: `journalctl -u monolab-relay -f`
