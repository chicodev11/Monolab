//! What the central server remembers for Monolink: people, their devices, teams, roles,
//! invites and a short activity log. Never server files, console text or commands.
//! See `docs/monolink.md` section 3.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};

use super::util::{DAY, Role, new_id, random};

/// How long a session stays valid without being used.
pub const SESSION_SECS: u64 = 30 * DAY;
/// Time between two handle changes, and how long a dropped handle stays reserved.
pub const HANDLE_COOLDOWN: u64 = 30 * DAY;
/// Teams one person may own, devices one person may have, open invites per team.
pub const TEAMS_PER_OWNER: i64 = 10;
pub const DEVICES_PER_USER: i64 = 10;
pub const INVITES_PER_TEAM: i64 = 50;
/// Activity rows older than this are dropped.
pub const ACTIVITY_KEEP: u64 = 90 * DAY;
/// Servers one team may have attached.
pub const SERVERS_PER_TEAM: i64 = 50;
/// Crash reports are dropped after this, and only this many are kept at all.
pub const CRASH_KEEP: u64 = 90 * DAY;
pub const CRASH_MOST: i64 = 20_000;
/// Daily usage rows (and installs not seen) older than this are dropped.
pub const INSTALL_KEEP_DAYS: u64 = 400;
/// How long a browser sign-in to the admin page lasts.
pub const ADMIN_SESSION_SECS: u64 = 12 * 3600;

/// What one app said in its `/v2/app` hello (with usage counts on). No IP, account or device.
#[derive(Clone, Debug, PartialEq)]
pub struct InstallSeen {
    pub id: String,
    pub app: String,
    pub addon: u64,
    pub fork: u64,
    pub os: String,
}

/// The numbers the admin page shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub today: u64,
    pub week: u64,
    pub month: u64,
    pub installs: u64,
    pub accounts: u64,
    pub devices: u64,
    /// Among installs seen in the last 30 days: (value, count), most first.
    pub by_app: Vec<(String, u64)>,
    pub by_addon: Vec<(String, u64)>,
    pub by_fork: Vec<(String, u64)>,
    pub by_os: Vec<(String, u64)>,
}

/// An app crash report someone chose to send. No account, device or address is stored with it.
#[derive(Clone, Debug, PartialEq)]
pub struct CrashReport {
    pub version: String,
    pub os: String,
    pub kind: String,
    pub message: String,
    pub detail: String,
}

/// A server attached to a team. The central server knows its name and where it runs, nothing inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct TeamServer {
    pub id: String,
    pub host_device: String,
    /// The member whose device runs it (None right after attaching).
    pub host_user: Option<String>,
    pub instance_id: String,
    pub name: String,
}

pub struct Store(Mutex<Connection>);

#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub id: String,
    pub handle: String,
    pub avatar: Option<String>,
    pub handle_changed_at: u64,
    /// Permanent 8-digit number, given once at sign-up and never changed (the handle can change).
    pub number: u64,
}

#[derive(Clone, Debug)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub last_seen: u64,
    /// "app" (a person's PC) or "node" (a monolab-node machine they paired).
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub version: u64,
}

#[derive(Clone, Debug)]
pub struct Member {
    pub user: User,
    pub role: Role,
}

#[derive(Clone, Debug)]
pub struct Invite {
    pub code_hash: Vec<u8>,
    pub prefix: String,
    pub team: String,
    pub role: Role,
    pub created_by: String,
    pub created_at: u64,
    pub expires: u64,
    pub max_uses: Option<u32>,
    pub uses: u32,
    pub needs_approval: bool,
    pub revoked: bool,
}

impl Invite {
    /// Why this invite can't be used now, if it can't.
    pub fn unusable(&self, now: u64) -> Option<&'static str> {
        if self.revoked {
            Some("revoked")
        } else if now >= self.expires {
            Some("expired")
        } else if self.max_uses.is_some_and(|most| self.uses >= most) {
            Some("used_up")
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub struct JoinRequest {
    pub user: User,
    pub role: Role,
    pub created_at: u64,
}

#[derive(Clone, Debug)]
pub struct Activity {
    pub id: i64,
    pub actor: Option<String>,
    pub kind: String,
    pub data: String,
    pub at: u64,
}

fn user_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        id: row.get(0)?,
        handle: row.get(1)?,
        avatar: row.get(2)?,
        handle_changed_at: row.get::<_, i64>(3)? as u64,
        number: row.get::<_, Option<i64>>(4)?.unwrap_or(0) as u64,
    })
}

fn role_of(text: String) -> rusqlite::Result<Role> {
    Role::parse(&text).ok_or(rusqlite::Error::InvalidQuery)
}

const USER_COLS: &str = "users.id, users.handle, users.avatar_url, users.handle_changed_at, users.number";

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let db = Connection::open(path)?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                handle TEXT NOT NULL UNIQUE,
                avatar_url TEXT,
                handle_changed_at INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                disabled_at INTEGER
            );
            CREATE TABLE IF NOT EXISTS oauth_identities (
                provider TEXT NOT NULL,
                subject TEXT NOT NULL,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (provider, subject)
            );
            CREATE TABLE IF NOT EXISTS handle_holds (
                handle TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                until INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS devices (
                id TEXT PRIMARY KEY,
                public_key BLOB NOT NULL,
                name TEXT NOT NULL,
                user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
                created_at INTEGER NOT NULL,
                last_seen INTEGER NOT NULL,
                revoked_at INTEGER
            );
            CREATE INDEX IF NOT EXISTS devices_user ON devices (user_id);
            CREATE TABLE IF NOT EXISTS sessions (
                token_hash BLOB PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
                created_at INTEGER NOT NULL,
                expires INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS sessions_device ON sessions (device_id);
            CREATE TABLE IF NOT EXISTS teams (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                owner_id TEXT NOT NULL REFERENCES users(id),
                version INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS members (
                team_id TEXT NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                role TEXT NOT NULL CHECK (role IN ('owner','admin','developer','viewer')),
                added_by TEXT,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (team_id, user_id)
            );
            CREATE INDEX IF NOT EXISTS members_user ON members (user_id);
            CREATE TABLE IF NOT EXISTS servers (
                id TEXT PRIMARY KEY,
                team_id TEXT NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
                host_device TEXT NOT NULL,
                instance_id TEXT NOT NULL,
                name TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                UNIQUE (host_device, instance_id)
            );
            CREATE TABLE IF NOT EXISTS invites (
                code_hash BLOB PRIMARY KEY,
                prefix TEXT NOT NULL,
                team_id TEXT NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
                role TEXT NOT NULL CHECK (role IN ('admin','developer','viewer')),
                created_by TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                expires INTEGER NOT NULL,
                max_uses INTEGER,
                uses INTEGER NOT NULL DEFAULT 0,
                needs_approval INTEGER NOT NULL DEFAULT 1,
                revoked_at INTEGER
            );
            CREATE INDEX IF NOT EXISTS invites_team ON invites (team_id);
            CREATE TABLE IF NOT EXISTS join_requests (
                team_id TEXT NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                code_hash BLOB NOT NULL,
                role TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (team_id, user_id)
            );
            CREATE TABLE IF NOT EXISTS activity (
                id INTEGER PRIMARY KEY,
                team_id TEXT NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
                server_id TEXT,
                actor_id TEXT,
                kind TEXT NOT NULL,
                data TEXT NOT NULL,
                at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS activity_team ON activity (team_id, id);
            CREATE TABLE IF NOT EXISTS direct_invites (
                team_id TEXT NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                role TEXT NOT NULL CHECK (role IN ('admin','developer','viewer')),
                invited_by TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                expires INTEGER NOT NULL,
                PRIMARY KEY (team_id, user_id)
            );
            CREATE INDEX IF NOT EXISTS direct_invites_user ON direct_invites (user_id);
            CREATE TABLE IF NOT EXISTS installs (
                id TEXT PRIMARY KEY,
                first_seen INTEGER NOT NULL,
                last_seen INTEGER NOT NULL,
                app TEXT NOT NULL,
                addon INTEGER NOT NULL,
                fork INTEGER NOT NULL,
                os TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS install_days (
                day INTEGER NOT NULL,
                id TEXT NOT NULL,
                PRIMARY KEY (day, id)
            ) WITHOUT ROWID;
            CREATE TABLE IF NOT EXISTS admin_sessions (
                token_hash BLOB PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                expires INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS crash_reports (
                id INTEGER PRIMARY KEY,
                version TEXT NOT NULL,
                os TEXT NOT NULL,
                kind TEXT NOT NULL,
                message TEXT NOT NULL,
                detail TEXT NOT NULL,
                at INTEGER NOT NULL
            );",
        )?;
        Self::give_numbers(&db)?;
        Self::give_kinds(&db)?;
        Ok(Self(Mutex::new(db)))
    }

    /// Older databases: adds the permanent `number` column and gives every person without one a number.
    /// Devices got a kind later: "app" for every device made before, "node" for paired monolab-node machines.
    fn give_kinds(db: &Connection) -> rusqlite::Result<()> {
        let has: bool =
            db.query_row("SELECT EXISTS (SELECT 1 FROM pragma_table_info('devices') WHERE name = 'kind')", [], |row| row.get(0))?;
        if !has {
            db.execute_batch("ALTER TABLE devices ADD COLUMN kind TEXT NOT NULL DEFAULT 'app'")?;
        }
        Ok(())
    }

    fn give_numbers(db: &Connection) -> rusqlite::Result<()> {
        let has: bool =
            db.query_row("SELECT EXISTS (SELECT 1 FROM pragma_table_info('users') WHERE name = 'number')", [], |row| row.get(0))?;
        if !has {
            db.execute_batch("ALTER TABLE users ADD COLUMN number INTEGER")?;
        }
        db.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS users_number ON users (number)")?;
        let missing: Vec<String> = {
            let mut query = db.prepare("SELECT id FROM users WHERE number IS NULL ORDER BY created_at")?;
            let rows = query.query_map([], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for id in missing {
            let number = Self::free_number(db)?;
            db.execute("UPDATE users SET number = ?2 WHERE id = ?1", params![id, number as i64])?;
        }
        Ok(())
    }

    /// A random unused 8-digit number (10000000 to 99999999).
    fn free_number(db: &Connection) -> rusqlite::Result<u64> {
        loop {
            let raw = u64::from_le_bytes(random::<8>());
            let number = 10_000_000 + raw % 90_000_000;
            let taken: bool =
                db.query_row("SELECT EXISTS (SELECT 1 FROM users WHERE number = ?1)", params![number as i64], |row| row.get(0))?;
            if !taken {
                return Ok(number);
            }
        }
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        self.0.lock().expect("accounts store poisoned")
    }

    // ── devices ─────────────────────────────────────────────────────────────

    /// Adds a device, or refreshes its name and last-seen time. Refused for a revoked device.
    pub fn device_seen(&self, id: &str, public_key: &[u8], name: &str, now: u64) -> rusqlite::Result<bool> {
        let db = self.db();
        let revoked: Option<Option<i64>> =
            db.query_row("SELECT revoked_at FROM devices WHERE id = ?1", params![id], |row| row.get(0)).optional()?;
        if matches!(revoked, Some(Some(_))) {
            return Ok(false);
        }
        db.execute(
            "INSERT INTO devices (id, public_key, name, created_at, last_seen) VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, last_seen = excluded.last_seen",
            params![id, public_key, name, now as i64],
        )?;
        Ok(true)
    }

    pub fn devices_of(&self, user: &str) -> rusqlite::Result<Vec<Device>> {
        let db = self.db();
        let mut query = db.prepare(
            "SELECT id, name, created_at, last_seen, kind FROM devices WHERE user_id = ?1 AND revoked_at IS NULL ORDER BY last_seen DESC",
        )?;
        let rows = query.query_map(params![user], |row| {
            Ok(Device {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get::<_, i64>(2)? as u64,
                last_seen: row.get::<_, i64>(3)? as u64,
                kind: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Marks a device as a paired monolab-node machine.
    pub fn set_node(&self, device: &str) -> rusqlite::Result<()> {
        self.db().execute("UPDATE devices SET kind = 'node' WHERE id = ?1", params![device])?;
        Ok(())
    }

    /// Whether `device` is a device of some account (app or node) that wasn't removed.
    pub fn device_active(&self, device: &str) -> rusqlite::Result<bool> {
        self.db().query_row("SELECT EXISTS (SELECT 1 FROM devices WHERE id = ?1 AND revoked_at IS NULL)", params![device], |row| row.get(0))
    }

    /// Whether `device` is a paired monolab-node machine (not revoked).
    pub fn is_node(&self, device: &str) -> rusqlite::Result<bool> {
        self.db().query_row(
            "SELECT EXISTS (SELECT 1 FROM devices WHERE id = ?1 AND kind = 'node' AND revoked_at IS NULL)",
            params![device],
            |row| row.get(0),
        )
    }

    /// Whether `device` is a node `user` paired and hasn't revoked.
    pub fn is_node_of(&self, device: &str, user: &str) -> rusqlite::Result<bool> {
        self.db().query_row(
            "SELECT EXISTS (SELECT 1 FROM devices WHERE id = ?1 AND user_id = ?2 AND kind = 'node' AND revoked_at IS NULL)",
            params![device, user],
            |row| row.get(0),
        )
    }

    /// Takes a device away from its owner: its sessions end and it can't sign in again.
    pub fn revoke_device(&self, user: &str, device: &str, now: u64) -> rusqlite::Result<bool> {
        let db = self.db();
        let changed = db.execute(
            "UPDATE devices SET revoked_at = ?3 WHERE id = ?1 AND user_id = ?2 AND revoked_at IS NULL",
            params![device, user, now as i64],
        )?;
        db.execute("DELETE FROM sessions WHERE device_id = ?1", params![device])?;
        if changed > 0 {
            db.execute("DELETE FROM servers WHERE host_device = ?1", params![device])?;
        }
        Ok(changed > 0)
    }

    // ── users and sign-in ──────────────────────────────────────────────────

    pub fn user_by_identity(&self, provider: &str, subject: &str) -> rusqlite::Result<Option<(User, bool)>> {
        self.db()
            .query_row(
                &format!(
                    "SELECT {USER_COLS}, users.disabled_at IS NOT NULL FROM oauth_identities
                     JOIN users ON users.id = oauth_identities.user_id WHERE provider = ?1 AND subject = ?2"
                ),
                params![provider, subject],
                |row| Ok((user_row(row)?, row.get::<_, bool>(5)?)),
            )
            .optional()
    }

    pub fn user(&self, id: &str) -> rusqlite::Result<Option<User>> {
        self.db().query_row(&format!("SELECT {USER_COLS} FROM users WHERE id = ?1"), params![id], user_row).optional()
    }

    pub fn user_by_handle(&self, handle: &str) -> rusqlite::Result<Option<User>> {
        self.db().query_row(&format!("SELECT {USER_COLS} FROM users WHERE handle = ?1"), params![handle], user_row).optional()
    }

    pub fn user_by_number(&self, number: u64) -> rusqlite::Result<Option<User>> {
        self.db().query_row(&format!("SELECT {USER_COLS} FROM users WHERE number = ?1"), params![number as i64], user_row).optional()
    }

    // ── direct invites (by handle or number) ─────────────────────────────────

    /// Invites a person straight to a team; they still have to accept. Replaces an older invite to the same team.
    pub fn invite_person(&self, team: &str, user: &str, role: Role, by: &str, now: u64, expires: u64) -> rusqlite::Result<()> {
        self.db().execute(
            "INSERT INTO direct_invites (team_id, user_id, role, invited_by, created_at, expires) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (team_id, user_id) DO UPDATE SET role = excluded.role, invited_by = excluded.invited_by,
               created_at = excluded.created_at, expires = excluded.expires",
            params![team, user, role.as_str(), by, now as i64, expires as i64],
        )?;
        Ok(())
    }

    /// Open invites for one person: (team, role, inviter's user id).
    pub fn invites_for(&self, user: &str, now: u64) -> rusqlite::Result<Vec<(Team, Role, String)>> {
        let db = self.db();
        let mut query = db.prepare(
            "SELECT teams.id, teams.name, teams.owner_id, teams.version, direct_invites.role, direct_invites.invited_by FROM direct_invites
             JOIN teams ON teams.id = direct_invites.team_id WHERE direct_invites.user_id = ?1 AND direct_invites.expires > ?2
             ORDER BY direct_invites.created_at",
        )?;
        let rows = query.query_map(params![user, now as i64], |row| {
            Ok((
                Team { id: row.get(0)?, name: row.get(1)?, owner: row.get(2)?, version: row.get::<_, i64>(3)? as u64 },
                role_of(row.get(4)?)?,
                row.get(5)?,
            ))
        })?;
        rows.collect()
    }

    /// Accepts or declines a direct invite. Gives the role, or None when there was no open invite.
    pub fn answer_invite(&self, team: &str, user: &str, accept: bool, now: u64) -> rusqlite::Result<Option<Role>> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let found: Option<(String, String)> = tx
            .query_row(
                "SELECT role, invited_by FROM direct_invites WHERE team_id = ?1 AND user_id = ?2 AND expires > ?3",
                params![team, user, now as i64],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        tx.execute("DELETE FROM direct_invites WHERE team_id = ?1 AND user_id = ?2", params![team, user])?;
        let role = found.as_ref().and_then(|(role, _)| Role::parse(role));
        if let (true, Some(role), Some((_, by))) = (accept, role, found.as_ref()) {
            tx.execute(
                "INSERT OR IGNORE INTO members (team_id, user_id, role, added_by, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![team, user, role.as_str(), by, now as i64],
            )?;
            tx.execute("DELETE FROM join_requests WHERE team_id = ?1 AND user_id = ?2", params![team, user])?;
        }
        tx.commit()?;
        Ok(role)
    }

    /// Whether a (valid, lowercase) handle is free for `user` (None for a new person).
    pub fn handle_free(&self, handle: &str, user: Option<&str>, now: u64) -> rusqlite::Result<bool> {
        let db = self.db();
        let taken: Option<String> = db.query_row("SELECT id FROM users WHERE handle = ?1", params![handle], |row| row.get(0)).optional()?;
        if taken.is_some() {
            return Ok(false);
        }
        let held: Option<String> = db
            .query_row("SELECT user_id FROM handle_holds WHERE handle = ?1 AND until > ?2", params![handle, now as i64], |row| row.get(0))
            .optional()?;
        // A person may take back the handle they just dropped.
        Ok(match held {
            None => true,
            Some(holder) => user == Some(holder.as_str()),
        })
    }

    /// Makes a new person with their first sign-in. Fails with `Ok(None)` when the handle was taken meanwhile.
    pub fn create_user(
        &self,
        handle: &str,
        avatar: Option<&str>,
        provider: &str,
        subject: &str,
        now: u64,
    ) -> rusqlite::Result<Option<User>> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let busy: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM users WHERE handle = ?1) OR EXISTS (SELECT 1 FROM handle_holds WHERE handle = ?1 AND until > ?2)",
            params![handle, now as i64],
            |row| row.get(0),
        )?;
        let known: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM oauth_identities WHERE provider = ?1 AND subject = ?2)",
            params![provider, subject],
            |row| row.get(0),
        )?;
        if busy || known {
            return Ok(None);
        }
        let id = new_id(now);
        let number = Self::free_number(&tx)?;
        tx.execute(
            "INSERT INTO users (id, handle, avatar_url, handle_changed_at, created_at, number) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
            params![id, handle, avatar, now as i64, number as i64],
        )?;
        tx.execute(
            "INSERT INTO oauth_identities (provider, subject, user_id, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![provider, subject, id, now as i64],
        )?;
        tx.commit()?;
        Ok(Some(User { id, handle: handle.into(), avatar: avatar.map(Into::into), handle_changed_at: now, number }))
    }

    pub fn set_avatar(&self, user: &str, avatar: Option<&str>) -> rusqlite::Result<()> {
        self.db().execute("UPDATE users SET avatar_url = ?2 WHERE id = ?1", params![user, avatar])?;
        Ok(())
    }

    /// Changes a handle and holds the old one. The caller checks the cooldown and that the new one is free.
    pub fn change_handle(&self, user: &User, handle: &str, now: u64) -> rusqlite::Result<bool> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let busy: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM users WHERE handle = ?1)
                 OR EXISTS (SELECT 1 FROM handle_holds WHERE handle = ?1 AND until > ?2 AND user_id <> ?3)",
            params![handle, now as i64, user.id],
            |row| row.get(0),
        )?;
        if busy {
            return Ok(false);
        }
        tx.execute("DELETE FROM handle_holds WHERE until <= ?1 OR handle = ?2", params![now as i64, handle])?;
        tx.execute(
            "INSERT OR REPLACE INTO handle_holds (handle, user_id, until) VALUES (?1, ?2, ?3)",
            params![user.handle, user.id, (now + HANDLE_COOLDOWN) as i64],
        )?;
        tx.execute("UPDATE users SET handle = ?2, handle_changed_at = ?3 WHERE id = ?1", params![user.id, handle, now as i64])?;
        tx.commit()?;
        Ok(true)
    }

    /// Removes a person. Refused (false) while they own a team.
    pub fn delete_user(&self, user: &User, now: u64) -> rusqlite::Result<bool> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let owns: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM teams WHERE owner_id = ?1)", params![user.id], |row| row.get(0))?;
        if owns {
            return Ok(false);
        }
        tx.execute(
            "INSERT OR REPLACE INTO handle_holds (handle, user_id, until) VALUES (?1, ?2, ?3)",
            params![user.handle, user.id, (now + HANDLE_COOLDOWN) as i64],
        )?;
        tx.execute("UPDATE devices SET user_id = NULL WHERE user_id = ?1", params![user.id])?;
        tx.execute("DELETE FROM users WHERE id = ?1", params![user.id])?;
        tx.commit()?;
        Ok(true)
    }

    // ── sessions ───────────────────────────────────────────────────────────

    /// Signs a device in as `user`. A device belongs to one person; signing in as someone else moves it
    /// and ends the old person's sessions on it. Refused (false) when the person has too many devices.
    pub fn open_session(&self, token_hash: &[u8], user: &str, device: &str, now: u64) -> rusqlite::Result<bool> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let owner: Option<Option<String>> =
            tx.query_row("SELECT user_id FROM devices WHERE id = ?1", params![device], |row| row.get(0)).optional()?;
        if owner.flatten().as_deref() != Some(user) {
            let count: i64 =
                tx.query_row("SELECT COUNT(*) FROM devices WHERE user_id = ?1 AND revoked_at IS NULL", params![user], |row| row.get(0))?;
            if count >= DEVICES_PER_USER {
                return Ok(false);
            }
            tx.execute("DELETE FROM sessions WHERE device_id = ?1", params![device])?;
            tx.execute("UPDATE devices SET user_id = ?2 WHERE id = ?1", params![device, user])?;
        }
        tx.execute(
            "INSERT INTO sessions (token_hash, user_id, device_id, created_at, expires) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![token_hash, user, device, now as i64, (now + SESSION_SECS) as i64],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// The person and device behind a session token, stretching the session when it's used.
    pub fn session(&self, token_hash: &[u8], now: u64) -> rusqlite::Result<Option<(User, String)>> {
        let db = self.db();
        let found: Option<(User, String, u64)> = db
            .query_row(
                &format!(
                    "SELECT {USER_COLS}, sessions.device_id, sessions.expires FROM sessions
                     JOIN users ON users.id = sessions.user_id
                     JOIN devices ON devices.id = sessions.device_id
                     WHERE sessions.token_hash = ?1 AND sessions.expires > ?2
                       AND users.disabled_at IS NULL AND devices.revoked_at IS NULL AND devices.user_id = users.id"
                ),
                params![token_hash, now as i64],
                |row| Ok((user_row(row)?, row.get(5)?, row.get::<_, i64>(6)? as u64)),
            )
            .optional()?;
        let Some((user, device, expires)) = found else { return Ok(None) };
        // Stretch at most once a day, so reads don't turn into writes.
        if expires < now + SESSION_SECS - DAY {
            db.execute("UPDATE sessions SET expires = ?2 WHERE token_hash = ?1", params![token_hash, (now + SESSION_SECS) as i64])?;
            db.execute("UPDATE devices SET last_seen = ?2 WHERE id = ?1", params![device, now as i64])?;
        }
        Ok(Some((user, device)))
    }

    pub fn close_session(&self, token_hash: &[u8]) -> rusqlite::Result<()> {
        self.db().execute("DELETE FROM sessions WHERE token_hash = ?1", params![token_hash])?;
        Ok(())
    }

    // ── teams ──────────────────────────────────────────────────────────────

    pub fn create_team(&self, owner: &str, name: &str, now: u64) -> rusqlite::Result<Option<Team>> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let owned: i64 = tx.query_row("SELECT COUNT(*) FROM teams WHERE owner_id = ?1", params![owner], |row| row.get(0))?;
        if owned >= TEAMS_PER_OWNER {
            return Ok(None);
        }
        let id = new_id(now);
        tx.execute("INSERT INTO teams (id, name, owner_id, created_at) VALUES (?1, ?2, ?3, ?4)", params![id, name, owner, now as i64])?;
        tx.execute(
            "INSERT INTO members (team_id, user_id, role, created_at) VALUES (?1, ?2, 'owner', ?3)",
            params![id, owner, now as i64],
        )?;
        tx.commit()?;
        Ok(Some(Team { id, name: name.into(), owner: owner.into(), version: 1 }))
    }

    pub fn team(&self, id: &str) -> rusqlite::Result<Option<Team>> {
        self.db()
            .query_row("SELECT id, name, owner_id, version FROM teams WHERE id = ?1", params![id], |row| {
                Ok(Team { id: row.get(0)?, name: row.get(1)?, owner: row.get(2)?, version: row.get::<_, i64>(3)? as u64 })
            })
            .optional()
    }

    /// Teams a person is in, with their role.
    pub fn teams_of(&self, user: &str) -> rusqlite::Result<Vec<(Team, Role)>> {
        let db = self.db();
        let mut query = db.prepare(
            "SELECT teams.id, teams.name, teams.owner_id, teams.version, members.role FROM members
             JOIN teams ON teams.id = members.team_id WHERE members.user_id = ?1 ORDER BY teams.created_at",
        )?;
        let rows = query.query_map(params![user], |row| {
            Ok((
                Team { id: row.get(0)?, name: row.get(1)?, owner: row.get(2)?, version: row.get::<_, i64>(3)? as u64 },
                role_of(row.get(4)?)?,
            ))
        })?;
        rows.collect()
    }

    pub fn role(&self, team: &str, user: &str) -> rusqlite::Result<Option<Role>> {
        let text: Option<String> = self
            .db()
            .query_row("SELECT role FROM members WHERE team_id = ?1 AND user_id = ?2", params![team, user], |row| row.get(0))
            .optional()?;
        Ok(text.and_then(|one| Role::parse(&one)))
    }

    pub fn members(&self, team: &str) -> rusqlite::Result<Vec<Member>> {
        let db = self.db();
        let mut query = db.prepare(&format!(
            "SELECT {USER_COLS}, members.role FROM members JOIN users ON users.id = members.user_id
             WHERE members.team_id = ?1 ORDER BY members.created_at"
        ))?;
        let rows = query.query_map(params![team], |row| Ok(Member { user: user_row(row)?, role: role_of(row.get(5)?)? }))?;
        rows.collect()
    }

    /// Everyone who shares at least one team with `user`, `user` included.
    pub fn teammates(&self, user: &str) -> rusqlite::Result<Vec<String>> {
        let db = self.db();
        let mut query = db.prepare(
            "SELECT DISTINCT other.user_id FROM members mine JOIN members other ON other.team_id = mine.team_id WHERE mine.user_id = ?1",
        )?;
        let rows = query.query_map(params![user], |row| row.get(0))?;
        rows.collect()
    }

    /// Marks a team as changed so hosts re-check what they allowed. Gives the new version.
    pub fn bump(&self, team: &str) -> rusqlite::Result<u64> {
        let version: i64 =
            self.db()
                .query_row("UPDATE teams SET version = version + 1 WHERE id = ?1 RETURNING version", params![team], |row| row.get(0))?;
        Ok(version as u64)
    }

    pub fn rename_team(&self, team: &str, name: &str) -> rusqlite::Result<()> {
        self.db().execute("UPDATE teams SET name = ?2 WHERE id = ?1", params![team, name])?;
        Ok(())
    }

    pub fn delete_team(&self, team: &str) -> rusqlite::Result<()> {
        self.db().execute("DELETE FROM teams WHERE id = ?1", params![team])?;
        Ok(())
    }

    pub fn set_role(&self, team: &str, user: &str, role: Role) -> rusqlite::Result<()> {
        self.db().execute("UPDATE members SET role = ?3 WHERE team_id = ?1 AND user_id = ?2", params![team, user, role.as_str()])?;
        Ok(())
    }

    /// Takes someone out of a team, with the servers their devices had attached to it.
    pub fn remove_member(&self, team: &str, user: &str) -> rusqlite::Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        tx.execute("DELETE FROM members WHERE team_id = ?1 AND user_id = ?2", params![team, user])?;
        tx.execute(
            "DELETE FROM servers WHERE team_id = ?1 AND host_device IN (SELECT id FROM devices WHERE user_id = ?2)",
            params![team, user],
        )?;
        tx.commit()
    }

    // ── servers attached to teams ──────────────────────────────────────────

    /// Attaches a server running on `host` to a team. None when that server is already in a team
    /// or the team is full.
    pub fn attach_server(&self, team: &str, host: &str, instance: &str, name: &str, now: u64) -> rusqlite::Result<Option<TeamServer>> {
        let db = self.db();
        let count: i64 = db.query_row("SELECT COUNT(*) FROM servers WHERE team_id = ?1", params![team], |row| row.get(0))?;
        if count >= SERVERS_PER_TEAM {
            return Ok(None);
        }
        let id = new_id(now);
        let added = db.execute(
            "INSERT OR IGNORE INTO servers (id, team_id, host_device, instance_id, name, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, team, host, instance, name, now as i64],
        )?;
        Ok((added > 0).then(|| TeamServer {
            id,
            host_device: host.into(),
            host_user: None,
            instance_id: instance.into(),
            name: name.into(),
        }))
    }

    pub fn detach_server(&self, team: &str, id: &str) -> rusqlite::Result<bool> {
        Ok(self.db().execute("DELETE FROM servers WHERE team_id = ?1 AND id = ?2", params![team, id])? > 0)
    }

    /// A team's servers whose host device still belongs to a member, with that member's id.
    pub fn team_servers(&self, team: &str) -> rusqlite::Result<Vec<TeamServer>> {
        let db = self.db();
        let mut query = db.prepare(
            "SELECT s.id, s.host_device, d.user_id, s.instance_id, s.name FROM servers s
             JOIN devices d ON d.id = s.host_device AND d.revoked_at IS NULL
             JOIN members m ON m.team_id = s.team_id AND m.user_id = d.user_id
             WHERE s.team_id = ?1 ORDER BY s.created_at",
        )?;
        let rows = query.query_map(params![team], |row| {
            Ok(TeamServer {
                id: row.get(0)?,
                host_device: row.get(1)?,
                host_user: row.get(2)?,
                instance_id: row.get(3)?,
                name: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Hands a team to `to` (who becomes owner); the old owner becomes an admin.
    pub fn transfer(&self, team: &str, from: &str, to: &str) -> rusqlite::Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        tx.execute("UPDATE teams SET owner_id = ?2 WHERE id = ?1", params![team, to])?;
        tx.execute("UPDATE members SET role = 'admin' WHERE team_id = ?1 AND user_id = ?2", params![team, from])?;
        tx.execute("UPDATE members SET role = 'owner' WHERE team_id = ?1 AND user_id = ?2", params![team, to])?;
        tx.commit()
    }

    // ── invites ────────────────────────────────────────────────────────────

    /// Opens an invite. Refused (false) when the team already has too many open.
    pub fn open_invite(&self, invite: &Invite, now: u64) -> rusqlite::Result<bool> {
        let db = self.db();
        let open: i64 = db.query_row(
            "SELECT COUNT(*) FROM invites WHERE team_id = ?1 AND revoked_at IS NULL AND expires > ?2 AND (max_uses IS NULL OR uses < max_uses)",
            params![invite.team, now as i64],
            |row| row.get(0),
        )?;
        if open >= INVITES_PER_TEAM {
            return Ok(false);
        }
        db.execute(
            "INSERT INTO invites (code_hash, prefix, team_id, role, created_by, created_at, expires, max_uses, needs_approval)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                invite.code_hash,
                invite.prefix,
                invite.team,
                invite.role.as_str(),
                invite.created_by,
                invite.created_at as i64,
                invite.expires as i64,
                invite.max_uses,
                invite.needs_approval
            ],
        )?;
        Ok(true)
    }

    fn invite_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Invite> {
        Ok(Invite {
            code_hash: row.get(0)?,
            prefix: row.get(1)?,
            team: row.get(2)?,
            role: role_of(row.get(3)?)?,
            created_by: row.get(4)?,
            created_at: row.get::<_, i64>(5)? as u64,
            expires: row.get::<_, i64>(6)? as u64,
            max_uses: row.get(7)?,
            uses: row.get(8)?,
            needs_approval: row.get(9)?,
            revoked: row.get::<_, Option<i64>>(10)?.is_some(),
        })
    }

    const INVITE_COLS: &str =
        "code_hash, prefix, team_id, role, created_by, created_at, expires, max_uses, uses, needs_approval, revoked_at";

    pub fn invite(&self, code_hash: &[u8]) -> rusqlite::Result<Option<Invite>> {
        self.db()
            .query_row(&format!("SELECT {} FROM invites WHERE code_hash = ?1", Self::INVITE_COLS), params![code_hash], Self::invite_row)
            .optional()
    }

    /// Invites of a team that can still be used, newest first.
    pub fn open_invites(&self, team: &str, now: u64) -> rusqlite::Result<Vec<Invite>> {
        let db = self.db();
        let mut query = db.prepare(&format!(
            "SELECT {} FROM invites WHERE team_id = ?1 AND revoked_at IS NULL AND expires > ?2
             AND (max_uses IS NULL OR uses < max_uses) ORDER BY created_at DESC",
            Self::INVITE_COLS
        ))?;
        let rows = query.query_map(params![team, now as i64], Self::invite_row)?;
        rows.collect()
    }

    pub fn revoke_invite(&self, team: &str, code_hash: &[u8], now: u64) -> rusqlite::Result<bool> {
        let changed = self.db().execute(
            "UPDATE invites SET revoked_at = ?3 WHERE team_id = ?1 AND code_hash = ?2 AND revoked_at IS NULL",
            params![team, code_hash, now as i64],
        )?;
        Ok(changed > 0)
    }

    /// Uses an invite once: adds the person to the team, or files a join request when it needs approval.
    /// Gives `None` when the invite stopped being usable meanwhile.
    pub fn use_invite(&self, code_hash: &[u8], user: &str, now: u64) -> rusqlite::Result<Option<Joined>> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let Some(invite) = tx
            .query_row(&format!("SELECT {} FROM invites WHERE code_hash = ?1", Self::INVITE_COLS), params![code_hash], Self::invite_row)
            .optional()?
        else {
            return Ok(None);
        };
        if invite.unusable(now).is_some() {
            return Ok(None);
        }
        let member: bool =
            tx.query_row("SELECT EXISTS (SELECT 1 FROM members WHERE team_id = ?1 AND user_id = ?2)", params![invite.team, user], |row| {
                row.get(0)
            })?;
        if member {
            return Ok(Some(Joined::Already(invite.team)));
        }
        let waiting: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM join_requests WHERE team_id = ?1 AND user_id = ?2)",
            params![invite.team, user],
            |row| row.get(0),
        )?;
        if waiting {
            return Ok(Some(Joined::Waiting(invite.team)));
        }
        tx.execute("UPDATE invites SET uses = uses + 1 WHERE code_hash = ?1", params![code_hash])?;
        let joined = if invite.needs_approval {
            tx.execute(
                "INSERT INTO join_requests (team_id, user_id, code_hash, role, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![invite.team, user, code_hash, invite.role.as_str(), now as i64],
            )?;
            Joined::Requested(invite.team)
        } else {
            tx.execute(
                "INSERT INTO members (team_id, user_id, role, added_by, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![invite.team, user, invite.role.as_str(), invite.created_by, now as i64],
            )?;
            Joined::Joined(invite.team, invite.role)
        };
        tx.commit()?;
        Ok(Some(joined))
    }

    pub fn join_requests(&self, team: &str) -> rusqlite::Result<Vec<JoinRequest>> {
        let db = self.db();
        let mut query = db.prepare(&format!(
            "SELECT {USER_COLS}, join_requests.role, join_requests.created_at FROM join_requests
             JOIN users ON users.id = join_requests.user_id WHERE join_requests.team_id = ?1 ORDER BY join_requests.created_at"
        ))?;
        let rows = query.query_map(params![team], |row| {
            Ok(JoinRequest { user: user_row(row)?, role: role_of(row.get(5)?)?, created_at: row.get::<_, i64>(6)? as u64 })
        })?;
        rows.collect()
    }

    /// Answers a join request. Gives the role the person joined with, or None when there was no request.
    pub fn answer_request(&self, team: &str, user: &str, accept: bool, by: &str, now: u64) -> rusqlite::Result<Option<Role>> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let role: Option<String> = tx
            .query_row("SELECT role FROM join_requests WHERE team_id = ?1 AND user_id = ?2", params![team, user], |row| row.get(0))
            .optional()?;
        let Some(role) = role.and_then(|one| Role::parse(&one)) else { return Ok(None) };
        tx.execute("DELETE FROM join_requests WHERE team_id = ?1 AND user_id = ?2", params![team, user])?;
        if accept {
            tx.execute(
                "INSERT OR IGNORE INTO members (team_id, user_id, role, added_by, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![team, user, role.as_str(), by, now as i64],
            )?;
        }
        tx.commit()?;
        Ok(Some(role))
    }

    // ── activity ───────────────────────────────────────────────────────────

    pub fn record(&self, team: &str, actor: Option<&str>, kind: &str, data: &serde_json::Value, now: u64) -> rusqlite::Result<Activity> {
        self.record_for(team, None, actor, kind, data, now)
    }

    /// An activity row, about one of the team's servers or none.
    pub fn record_for(
        &self,
        team: &str,
        server: Option<&str>,
        actor: Option<&str>,
        kind: &str,
        data: &serde_json::Value,
        now: u64,
    ) -> rusqlite::Result<Activity> {
        let db = self.db();
        let _ = db.execute("DELETE FROM activity WHERE at < ?1", params![now.saturating_sub(ACTIVITY_KEEP) as i64]);
        let text = data.to_string();
        db.execute(
            "INSERT INTO activity (team_id, server_id, actor_id, kind, data, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![team, server, actor, kind, text, now as i64],
        )?;
        Ok(Activity { id: db.last_insert_rowid(), actor: actor.map(Into::into), kind: kind.into(), data: text, at: now })
    }

    // ── crash reports ──────────────────────────────────────────────────────

    pub fn add_crash(&self, report: &CrashReport, now: u64) -> rusqlite::Result<i64> {
        let db = self.db();
        db.execute("DELETE FROM crash_reports WHERE at < ?1", params![now.saturating_sub(CRASH_KEEP) as i64])?;
        db.execute(
            "DELETE FROM crash_reports WHERE id IN (SELECT id FROM crash_reports ORDER BY id DESC LIMIT -1 OFFSET ?1)",
            params![CRASH_MOST - 1],
        )?;
        db.execute(
            "INSERT INTO crash_reports (version, os, kind, message, detail, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![report.version, report.os, report.kind, report.message, report.detail, now as i64],
        )?;
        Ok(db.last_insert_rowid())
    }

    /// The newest crash reports, for whoever runs the server (`sqlite3 accounts.db` works too).
    pub fn crashes(&self, most: u32) -> rusqlite::Result<Vec<(i64, CrashReport, u64)>> {
        let db = self.db();
        let mut query = db.prepare("SELECT id, version, os, kind, message, detail, at FROM crash_reports ORDER BY id DESC LIMIT ?1")?;
        let rows = query.query_map(params![most], |row| {
            Ok((
                row.get(0)?,
                CrashReport { version: row.get(1)?, os: row.get(2)?, kind: row.get(3)?, message: row.get(4)?, detail: row.get(5)? },
                row.get::<_, i64>(6)? as u64,
            ))
        })?;
        rows.collect()
    }

    // ── anonymous usage counts (/v2/app) ───────────────────────────────────

    /// One app said hello with usage counts on: remember its versions and that it was used today (UTC).
    pub fn install_seen(&self, seen: &InstallSeen, now: u64) -> rusqlite::Result<()> {
        let db = self.db();
        db.execute(
            "INSERT INTO installs (id, first_seen, last_seen, app, addon, fork, os) VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (id) DO UPDATE SET last_seen = excluded.last_seen, app = excluded.app, addon = excluded.addon,
             fork = excluded.fork, os = excluded.os",
            params![seen.id, now as i64, seen.app, seen.addon as i64, seen.fork as i64, seen.os],
        )?;
        db.execute("INSERT OR IGNORE INTO install_days (day, id) VALUES (?1, ?2)", params![(now / DAY) as i64, seen.id])?;
        Ok(())
    }

    /// Drops daily rows older than `INSTALL_KEEP_DAYS`, installs not seen for that long, and ended admin
    /// sign-ins. Gives how many daily rows went.
    pub fn prune_usage(&self, now: u64) -> rusqlite::Result<usize> {
        let db = self.db();
        let first_day = (now / DAY).saturating_sub(INSTALL_KEEP_DAYS) as i64;
        let gone = db.execute("DELETE FROM install_days WHERE day < ?1", params![first_day])?;
        db.execute("DELETE FROM installs WHERE last_seen < ?1", params![now.saturating_sub(INSTALL_KEEP_DAYS * DAY) as i64])?;
        db.execute("DELETE FROM admin_sessions WHERE expires <= ?1", params![now as i64])?;
        Ok(gone)
    }

    pub fn stats(&self, now: u64) -> rusqlite::Result<Stats> {
        let db = self.db();
        let today = (now / DAY) as i64;
        let count = |sql: &str, value: i64| -> rusqlite::Result<u64> {
            db.query_row(sql, params![value], |row| row.get::<_, i64>(0)).map(|one| one as u64)
        };
        let total = |sql: &str| -> rusqlite::Result<u64> { db.query_row(sql, [], |row| row.get::<_, i64>(0)).map(|one| one as u64) };
        let users_since = "SELECT COUNT(DISTINCT id) FROM install_days WHERE day >= ?1";
        let recent = now.saturating_sub(30 * DAY) as i64;
        let group = |column: &str| -> rusqlite::Result<Vec<(String, u64)>> {
            let mut query = db.prepare(&format!(
                "SELECT CAST({column} AS TEXT), COUNT(*) FROM installs WHERE last_seen >= ?1 GROUP BY {column} ORDER BY COUNT(*) DESC, {column} DESC LIMIT 50"
            ))?;
            let rows = query.query_map(params![recent], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64)))?;
            rows.collect()
        };
        Ok(Stats {
            today: count(users_since, today)?,
            week: count(users_since, today - 6)?,
            month: count(users_since, today - 29)?,
            installs: count("SELECT COUNT(*) FROM installs WHERE last_seen >= ?1", recent)?,
            accounts: total("SELECT COUNT(*) FROM users WHERE disabled_at IS NULL")?,
            devices: total("SELECT COUNT(*) FROM devices WHERE user_id IS NOT NULL AND revoked_at IS NULL")?,
            by_app: group("app")?,
            by_addon: group("addon")?,
            by_fork: group("fork")?,
            by_os: group("os")?,
        })
    }

    // ── admin page sign-ins ────────────────────────────────────────────────

    pub fn open_admin_session(&self, token_hash: &[u8], user: &str, now: u64) -> rusqlite::Result<()> {
        let db = self.db();
        db.execute("DELETE FROM admin_sessions WHERE expires <= ?1", params![now as i64])?;
        db.execute(
            "INSERT INTO admin_sessions (token_hash, user_id, expires) VALUES (?1, ?2, ?3)",
            params![token_hash, user, (now + ADMIN_SESSION_SECS) as i64],
        )?;
        Ok(())
    }

    /// The person behind an admin sign-in that hasn't ended (and whose account isn't disabled).
    pub fn admin_session(&self, token_hash: &[u8], now: u64) -> rusqlite::Result<Option<User>> {
        self.db()
            .query_row(
                &format!(
                    "SELECT {USER_COLS} FROM admin_sessions JOIN users ON users.id = admin_sessions.user_id
                     WHERE admin_sessions.token_hash = ?1 AND admin_sessions.expires > ?2 AND users.disabled_at IS NULL"
                ),
                params![token_hash, now as i64],
                user_row,
            )
            .optional()
    }

    pub fn close_admin_session(&self, token_hash: &[u8]) -> rusqlite::Result<()> {
        self.db().execute("DELETE FROM admin_sessions WHERE token_hash = ?1", params![token_hash])?;
        Ok(())
    }

    /// Up to `most` rows of a team's activity before `before` (or the newest), newest first.
    pub fn activity(&self, team: &str, before: Option<i64>, most: u32) -> rusqlite::Result<Vec<Activity>> {
        let db = self.db();
        let mut query =
            db.prepare("SELECT id, actor_id, kind, data, at FROM activity WHERE team_id = ?1 AND id < ?2 ORDER BY id DESC LIMIT ?3")?;
        let rows = query.query_map(params![team, before.unwrap_or(i64::MAX), most], |row| {
            Ok(Activity { id: row.get(0)?, actor: row.get(1)?, kind: row.get(2)?, data: row.get(3)?, at: row.get::<_, i64>(4)? as u64 })
        })?;
        rows.collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Joined {
    Joined(String, Role),
    /// A join request was just filed.
    Requested(String),
    /// A join request was already waiting.
    Waiting(String),
    Already(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("monolab-store-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (Store::open(&dir.join("accounts.db")).unwrap(), dir)
    }

    fn seen(id: &str) -> InstallSeen {
        InstallSeen { id: id.into(), app: "1.0.2".into(), addon: 41, fork: 0, os: "windows-x86_64".into() }
    }

    #[test]
    fn install_days_are_counted_and_pruned() {
        let (store, dir) = temp_store("prune");
        let today = 20_000 * DAY + 3600;
        let a = "a".repeat(32);
        let b = "b".repeat(32);
        // a: used 401 days ago and today; b: only 401 days ago.
        store.install_seen(&seen(&a), today - 401 * DAY).unwrap();
        store.install_seen(&seen(&b), today - 401 * DAY).unwrap();
        store.install_seen(&seen(&a), today).unwrap();
        store.install_seen(&seen(&a), today + 60).unwrap(); // same day: one row
        let days: i64 = store.db().query_row("SELECT COUNT(*) FROM install_days", [], |row| row.get(0)).unwrap();
        assert_eq!(days, 3);
        let stats = store.stats(today).unwrap();
        assert_eq!((stats.today, stats.week, stats.month, stats.installs), (1, 1, 1, 1));
        assert_eq!(stats.by_addon, vec![("41".to_string(), 1)]);

        assert_eq!(store.prune_usage(today).unwrap(), 2);
        let days: Vec<(i64, String)> = {
            let db = store.db();
            let mut query = db.prepare("SELECT day, id FROM install_days").unwrap();
            query.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap()
        };
        assert_eq!(days, vec![((today / DAY) as i64, a.clone())]);
        let installs: Vec<String> = {
            let db = store.db();
            let mut query = db.prepare("SELECT id FROM installs").unwrap();
            query.query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
        };
        assert_eq!(installs, vec![a]);
        // Exactly 400 days back stays.
        store.install_seen(&seen(&b), today - 400 * DAY).unwrap();
        assert_eq!(store.prune_usage(today).unwrap(), 0);
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }
}
