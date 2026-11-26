
use argon2::{
    password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier},
    Argon2,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

pub const ADMIN: &str = "Admin";
pub const MANAGER: &str = "Manager";
/// Can open the servers it was given and look at everything there, but change nothing.
pub const VIEWER: &str = "Viewer";

pub fn is_role(role: &str) -> bool {
    role == ADMIN || role == MANAGER || role == VIEWER
}

/// What a role may do on the servers it was given, as the host ticks them. Opening a server and
/// looking at its overview (status, players online, settings) needs none of these.
pub const PERMS: &[&str] = &[
    "console.view",
    "console.send",
    "power",
    "files.read",
    "files.write",
    "secrets",
    "plugins",
    "backups",
    "studio.read",
    "studio.write",
    "players",
    "settings",
];

/// What a Viewer has: watching, never changing, and secrets stay hidden.
const VIEWER_PERMS: &[&str] = &["console.view", "files.read", "studio.read"];

/// Custom role ids start with this, so they never clash with the built-in names.
pub const CUSTOM_PREFIX: &str = "role:";

/// A role the host made, with the permissions they ticked. Accounts and people store its `id`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CustomRole {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub perms: Vec<String>,
}

/// The permissions a role has. Admin and Manager have them all (Admin also has every server and the
/// app-wide commands, checked apart). A role nobody knows has none.
pub fn perms_of(role: &str, custom: &[CustomRole]) -> Vec<String> {
    match role {
        ADMIN | MANAGER => PERMS.iter().map(|one| one.to_string()).collect(),
        VIEWER => VIEWER_PERMS.iter().map(|one| one.to_string()).collect(),
        _ => custom
            .iter()
            .find(|one| one.id == role)
            .map(|one| PERMS.iter().filter(|perm| one.perms.iter().any(|mine| mine == *perm)).map(|perm| perm.to_string()).collect())
            .unwrap_or_default(),
    }
}

/// A role that can be given to someone on chosen servers: Manager, Viewer, or a custom role that exists.
pub fn is_server_role(role: &str, custom: &[CustomRole]) -> bool {
    role == MANAGER || role == VIEWER || custom.iter().any(|one| one.id == role)
}

/// Checks a custom role before saving: a name of its own (not a built-in one) and known permissions.
pub fn clean_role(id: &str, name: &str, perms: &[String], others: &[CustomRole]) -> Result<CustomRole, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 32 || name.chars().any(|one| one.is_control()) {
        return Err("Pick a role name of 1 to 32 characters.".into());
    }
    let taken = [ADMIN, MANAGER, VIEWER, "Owner", "Developer"].iter().any(|one| one.eq_ignore_ascii_case(name))
        || others.iter().any(|one| one.id != id && one.name.trim().eq_ignore_ascii_case(name));
    if taken {
        return Err("A role with that name already exists.".into());
    }
    if let Some(unknown) = perms.iter().find(|one| !PERMS.contains(&one.as_str())) {
        return Err(format!("Unknown permission: {unknown}"));
    }
    let perms = PERMS.iter().filter(|one| perms.iter().any(|mine| mine == *one)).map(|one| one.to_string()).collect();
    Ok(CustomRole { id: id.to_string(), name: name.to_string(), perms })
}

/// Whether `to` has every permission `from` has, and at least one more.
pub fn raises(from: &str, to: &str, custom: &[CustomRole]) -> bool {
    let before = perms_of(from, custom);
    let after = perms_of(to, custom);
    before.iter().all(|one| after.contains(one)) && after.len() > before.len()
}

/// How long someone counts as online after their last request.
pub const ONLINE_FOR: Duration = Duration::from_secs(45);

const SESSION_LIFE: Duration = Duration::from_secs(12 * 60 * 60);

const MAX_TRIES: u32 = 8;

const LOCKOUT: Duration = Duration::from_secs(10 * 60);

/// Failed sign-ins from one address before it is locked out for [`LOCKOUT`].
const IP_TRIES: u32 = 30;

/// After [`MAX_TRIES`] wrong passwords, one try per account name this often.
const NAME_WAIT: Duration = Duration::from_secs(5);

const DECOY_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$jAY3KZxbc2chhi0relMV9A$SfZqMCtHgTHjHN2XCCo/PZklRp/kuDTfRWaq054M9es";

fn digest(token: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Clone, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub name: String,
    pub hash: String,
    pub role: String,
    #[serde(default)]
    pub servers: Vec<String>,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("name", &self.name)
            .field("hash", &"[redacted]")
            .field("role", &self.role)
            .field("servers", &self.servers)
            .finish()
    }
}

impl Account {
    pub fn is_admin(&self) -> bool {
        self.role == ADMIN
    }

    /// May open and look at this server.
    pub fn may(&self, server_id: &str) -> bool {
        self.is_admin() || self.servers.iter().any(|one| one == server_id)
    }


}

#[derive(Clone, Debug)]
struct Session {
    account: Account,
    until: Instant,
    seen: Instant,
    /// The server this session last asked about.
    server: String,
}

/// Someone signed in to the web panel right now.
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Online {
    pub name: String,
    pub role: String,
    /// The server they're on, or empty on the server list.
    pub server: String,
    pub seconds_ago: u32,
}

#[derive(Default)]
pub struct Guard {
    sessions: Mutex<HashMap<String, Session>>,
    tries: Mutex<HashMap<String, (u32, Option<Instant>)>>,
}

impl Guard {
    pub fn hash(password: &str) -> Result<String, String> {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|hashed| hashed.to_string())
            .map_err(|error| format!("Could not save the password: {error}"))
    }

    #[cfg(test)]
    pub fn login(&self, accounts: &[Account], name: &str, password: &str) -> Result<String, String> {
        self.login_from(accounts, name, password, None)
    }

    /// Signs in. Wrong passwords are limited two ways: an address (other than this PC, which is also
    /// where a playit tunnel comes from) is locked out after [`IP_TRIES`]; an account name after
    /// [`MAX_TRIES`] only gets one try every [`NAME_WAIT`] — slow enough to stop guessing, but nobody
    /// can lock the owner out of their own account by failing on purpose.
    pub fn login_from(&self, accounts: &[Account], name: &str, password: &str, from: Option<std::net::IpAddr>) -> Result<String, String> {
        let name = name.trim();
        let address = from.filter(|ip| !ip.is_loopback()).map(|ip| format!("ip:{ip}"));
        {
            let mut tries = self.tries.lock().expect("auth poisoned");
            if let Some(address) = &address {
                let one = tries.entry(address.clone()).or_insert((0, None));
                if let Some(since) = one.1 {
                    if since.elapsed() < LOCKOUT {
                        let left = (LOCKOUT - since.elapsed()).as_secs() / 60 + 1;
                        return Err(format!("Too many failed attempts from this address. Try again in {left} minutes."));
                    }
                    *one = (0, None);
                }
            }
            let one = tries.entry(name.to_string()).or_insert((0, None));
            if one.0 >= MAX_TRIES && one.1.is_some_and(|last| last.elapsed() < NAME_WAIT) {
                return Err("Too many failed attempts. Wait a few seconds and try again.".into());
            }
            if one.0 >= MAX_TRIES {
                one.1 = Some(Instant::now());
            }
        }
        let found = accounts.iter().find(|one| one.name == name);
        let against = found.map(|account| account.hash.as_str()).unwrap_or(DECOY_HASH);
        let verified = PasswordHash::new(against).is_ok_and(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        });
        let ok = found.is_some() && verified;
        if !ok {
            let mut tries = self.tries.lock().expect("auth poisoned");
            let one = tries.entry(name.to_string()).or_insert((0, None));
            one.0 += 1;
            if one.0 >= MAX_TRIES {
                one.1 = Some(Instant::now());
            }
            if let Some(address) = address {
                let one = tries.entry(address).or_insert((0, None));
                one.0 += 1;
                if one.0 >= IP_TRIES {
                    one.1 = Some(Instant::now());
                }
            }
            return Err("Wrong name or password.".into());
        }
        self.tries.lock().expect("auth poisoned").remove(name);
        let token = super::make_key();
        self.sessions.lock().expect("auth poisoned").insert(
            digest(&token),
            Session {
                account: found.expect("Marked as matching, so it must exist").clone(),
                until: Instant::now() + SESSION_LIFE,
                seen: Instant::now(),
                server: String::new(),
            },
        );
        Ok(token)
    }

    /// Notes that a session is active, and on which server when the request names one.
    pub fn touch(&self, token: &str, server: Option<&str>) {
        if token.is_empty() {
            return;
        }
        let mut sessions = self.sessions.lock().expect("auth poisoned");
        if let Some(one) = sessions.get_mut(&digest(token)) {
            one.seen = Instant::now();
            if let Some(server) = server {
                one.server = server.to_string();
            }
        }
    }

    /// Who used the panel in the last [`ONLINE_FOR`], one row per account (their most recent session).
    pub fn online(&self) -> Vec<Online> {
        let sessions = self.sessions.lock().expect("auth poisoned");
        let now = Instant::now();
        let mut out: Vec<Online> = Vec::new();
        for one in sessions.values().filter(|one| one.until > now && now.duration_since(one.seen) < ONLINE_FOR) {
            let ago = now.duration_since(one.seen).as_secs() as u32;
            match out.iter_mut().find(|row| row.name == one.account.name) {
                Some(row) if row.seconds_ago <= ago => {}
                Some(row) => {
                    row.server = one.server.clone();
                    row.seconds_ago = ago;
                }
                None => out.push(Online { name: one.account.name.clone(), role: one.account.role.clone(), server: one.server.clone(), seconds_ago: ago }),
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn whose(&self, token: &str) -> Option<Account> {
        if token.is_empty() {
            return None;
        }
        let mut sessions = self.sessions.lock().expect("auth poisoned");
        let now = Instant::now();
        sessions.retain(|_, one| one.until > now);
        sessions.get(&digest(token)).map(|one| one.account.clone())
    }

    pub fn viewers(&self) -> usize {
        let mut sessions = self.sessions.lock().expect("auth poisoned");
        let now = Instant::now();
        sessions.retain(|_, one| one.until > now);
        sessions.len()
    }

    pub fn forget(&self, token: &str) {
        self.sessions.lock().expect("auth poisoned").remove(&digest(token));
    }

    pub fn forget_all(&self) {
        self.sessions.lock().expect("auth poisoned").clear();
    }
}

/// How long an invite link works.
pub const INVITE_LIFE_SECS: u64 = 7 * 24 * 60 * 60;

/// A one-time link that lets someone make their own account for one server.
/// Only a digest of the token is kept, like a password.
#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    /// Short public id for listing and revoking: the first 12 characters of the digest.
    pub id: String,
    #[serde(default)]
    pub digest: String,
    pub server: String,
    pub role: String,
    /// Unix seconds.
    pub expires: u64,
}

/// Makes an invite. Returns the token to put in the link (shown once) and what to store.
pub fn new_invite(server: &str, role: &str, now: u64, custom: &[CustomRole]) -> Result<(String, Invite), String> {
    if !is_server_role(role, custom) {
        return Err("Invite someone as Manager, Viewer or one of your own roles.".into());
    }
    let token = super::make_key();
    let hashed = digest(&token);
    Ok((token, Invite { id: hashed[..12].to_string(), digest: hashed, server: server.into(), role: role.into(), expires: now + INVITE_LIFE_SECS }))
}

/// The live invite a token belongs to, if any.
pub fn find_invite<'a>(invites: &'a [Invite], token: &str, now: u64) -> Option<&'a Invite> {
    if token.len() != 64 {
        return None;
    }
    let hashed = digest(token);
    invites.iter().find(|one| one.digest == hashed && one.expires > now)
}

/// Uses an invite: makes a new account, or gives an existing account (right password) the server too.
/// An invite only ever raises an existing account: its role changes only when the invite's role has
/// everything the old one had and more. Admins stay Admins.
pub fn accept_invite(accounts: &mut Vec<Account>, invite: &Invite, name: &str, password: &str, custom: &[CustomRole]) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 32 || name.chars().any(|one| one.is_control()) {
        return Err("Pick a name of 1 to 32 characters.".into());
    }
    if let Some(found) = accounts.iter_mut().find(|one| one.name == name) {
        let verified = PasswordHash::new(&found.hash).is_ok_and(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok());
        if !verified {
            return Err("That name is taken. If it's yours, enter its password.".into());
        }
        if !found.is_admin() && !found.servers.iter().any(|one| one == &invite.server) {
            found.servers.push(invite.server.clone());
        }
        if !found.is_admin() && raises(&found.role, &invite.role, custom) {
            found.role = invite.role.clone();
        }
        return Ok(());
    }
    if password.chars().count() < 8 {
        return Err("Passwords must be at least 8 characters.".into());
    }
    accounts.push(Account { name: name.into(), hash: Guard::hash(password)?, role: invite.role.clone(), servers: vec![invite.server.clone()] });
    Ok(())
}

#[cfg(test)]
mod collab_tests {
    use super::*;

    fn viewer() -> Account {
        Account { name: "v".into(), hash: Guard::hash("viewerpassword").unwrap(), role: VIEWER.into(), servers: vec!["s1".into()] }
    }

    #[test]
    fn viewers_look_but_dont_touch() {
        let one = viewer();
        assert!(one.may("s1"));
        assert!(!one.may("s2"));
        assert!(!perms_of(VIEWER, &[]).iter().any(|perm| perm.ends_with(".write") || perm == "secrets"));
        let manager = Account { role: MANAGER.into(), ..one.clone() };
        assert!(manager.may("s1") && !manager.may("s2"));
        assert_eq!(perms_of(MANAGER, &[]).len(), PERMS.len());
        let admin = Account { role: ADMIN.into(), servers: Vec::new(), ..one };
        assert!(admin.may("anything"));
    }

    #[test]
    fn invites_work_once_per_token_and_expire() {
        let (token, invite) = new_invite("s1", VIEWER, 1_000, &[]).unwrap();
        assert_eq!(token.len(), 64);
        assert!(!invite.digest.contains(&token));
        let list = vec![invite.clone()];
        assert!(find_invite(&list, &token, 1_001).is_some());
        assert!(find_invite(&list, &token, 1_000 + INVITE_LIFE_SECS).is_none(), "expired");
        assert!(find_invite(&list, &super::super::make_key(), 1_001).is_none());
        assert!(find_invite(&list, "short", 1_001).is_none());
        assert!(new_invite("s1", ADMIN, 0, &[]).is_err(), "no admin invites");
    }

    #[test]
    fn accepting_makes_or_extends_an_account() {
        let (_, invite) = new_invite("s2", MANAGER, 0, &[]).unwrap();
        let mut accounts = vec![viewer()];
        assert!(accept_invite(&mut accounts, &invite, "newbie", "short", &[]).is_err());
        accept_invite(&mut accounts, &invite, "newbie", "longenoughpw", &[]).unwrap();
        let made = accounts.iter().find(|one| one.name == "newbie").unwrap();
        assert_eq!((made.role.as_str(), made.servers.clone()), (MANAGER, vec!["s2".to_string()]));

        assert!(accept_invite(&mut accounts, &invite, "v", "wrongpassword", &[]).is_err(), "someone else's name");
        accept_invite(&mut accounts, &invite, "v", "viewerpassword", &[]).unwrap();
        let grown = accounts.iter().find(|one| one.name == "v").unwrap();
        assert_eq!(grown.servers, vec!["s1".to_string(), "s2".to_string()]);
        assert_eq!(grown.role, MANAGER, "a Manager invite raises a Viewer");
        assert_eq!(accounts.len(), 2);
    }

    fn helper() -> CustomRole {
        CustomRole { id: "role:abc".into(), name: "Helper".into(), perms: vec!["console.view".into(), "console.send".into(), "power".into()] }
    }

    #[test]
    fn custom_roles_have_only_what_was_ticked() {
        let custom = vec![helper()];
        assert_eq!(perms_of("role:abc", &custom), vec!["console.view", "console.send", "power"]);
        assert!(perms_of("role:gone", &custom).is_empty(), "a missing role has nothing");
        assert!(perms_of("", &custom).is_empty());
        let odd = CustomRole { perms: vec!["power".into(), "root".into()], ..helper() };
        assert_eq!(perms_of("role:abc", &[odd]), vec!["power"], "unknown permissions are ignored");
        assert!(is_server_role("role:abc", &custom) && is_server_role(VIEWER, &custom));
        assert!(!is_server_role(ADMIN, &custom) && !is_server_role("role:gone", &custom));
    }

    #[test]
    fn role_names_are_checked() {
        let custom = vec![helper()];
        assert!(clean_role("role:new", "manager", &[], &custom).is_err(), "built-in name");
        assert!(clean_role("role:new", " helper ", &[], &custom).is_err(), "taken");
        assert!(clean_role("role:abc", "Helper", &[], &custom).is_ok(), "its own name");
        assert!(clean_role("role:new", "", &[], &custom).is_err());
        assert!(clean_role("role:new", "Ops", &["root".into()], &custom).is_err());
        let made = clean_role("role:new", " Ops ", &["power".into(), "console.view".into(), "power".into()], &custom).unwrap();
        assert_eq!((made.name.as_str(), made.perms.clone()), ("Ops", vec!["console.view".to_string(), "power".to_string()]));
    }

    #[test]
    fn invites_can_use_custom_roles_and_only_raise() {
        let custom = vec![helper()];
        assert!(new_invite("s1", "role:abc", 0, &custom).is_ok());
        assert!(new_invite("s1", "role:gone", 0, &custom).is_err());
        let (_, invite) = new_invite("s2", "role:abc", 0, &custom).unwrap();
        let mut accounts = vec![viewer()];
        accept_invite(&mut accounts, &invite, "v", "viewerpassword", &custom).unwrap();
        assert_eq!(accounts[0].role, VIEWER, "Helper lacks files.read, so it doesn't raise a Viewer");
        accounts.push(Account { name: "m".into(), role: MANAGER.into(), ..viewer() });
        accept_invite(&mut accounts, &invite, "m", "viewerpassword", &custom).unwrap();
        assert_eq!(accounts[1].role, MANAGER, "never lowers");
        let mut small = vec![Account { role: "role:abc".into(), ..viewer() }];
        let (_, up) = new_invite("s3", MANAGER, 0, &custom).unwrap();
        accept_invite(&mut small, &up, "v", "viewerpassword", &custom).unwrap();
        assert_eq!(small[0].role, MANAGER, "Manager has everything Helper has");
        accept_invite(&mut accounts, &invite, "fresh", "longenoughpw", &custom).unwrap();
        assert_eq!(accounts[2].role, "role:abc");
    }

    #[test]
    fn shows_who_is_online() {
        let guard = Guard::default();
        let list = vec![viewer()];
        let token = guard.login(&list, "v", "viewerpassword").unwrap();
        guard.touch(&token, Some("s1"));
        let online = guard.online();
        assert_eq!(online.len(), 1);
        assert_eq!((online[0].name.as_str(), online[0].server.as_str()), ("v", "s1"));
        guard.forget(&token);
        assert!(guard.online().is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accounts() -> Vec<Account> {
        vec![
            Account {
                name: "na".into(),
                hash: Guard::hash("longpassword12").unwrap(),
                role: ADMIN.into(),
                servers: Vec::new(),
            },
            Account {
                name: "friend".into(),
                hash: Guard::hash("friendpassword").unwrap(),
                role: MANAGER.into(),
                servers: vec!["server1".into()],
            },
        ]
    }

    #[test]
    fn only_lets_the_right_password_through() {
        let guard = Guard::default();
        let list = accounts();
        assert!(guard.login(&list, "na", "wrong").is_err());
        let missing = guard.login(&list, "nobody", "anything").unwrap_err();
        let wrong = guard.login(&list, "na", "wrongagain").unwrap_err();
        assert_eq!(missing, wrong, "must not reveal whether a name exists");

        let token = guard.login(&list, "na", "longpassword12").unwrap();
        assert_eq!(guard.whose(&token).unwrap().name, "na");
        assert!(guard.whose("anything").is_none());
        assert!(guard.whose("").is_none());
        guard.forget(&token);
        assert!(guard.whose(&token).is_none());
    }

    #[test]
    fn manager_only_touches_what_it_was_given() {
        let list = accounts();
        let admin = &list[0];
        let manager = &list[1];
        assert!(admin.may("server1") && admin.may("server2"), "admins can do everything");
        assert!(manager.may("server1"), "assigned servers should work");
        assert!(!manager.may("server2"), "unassigned servers should be refused");
        let empty = Account {
            role: MANAGER.into(),
            servers: Vec::new(),
            ..manager.clone()
        };
        assert!(!empty.may("server1"));
    }

    #[test]
    fn unknown_names_take_as_long_as_wrong_passwords() {
        let guard = Guard::default();
        let list = accounts();
        let time = |name: &str| {
            let start = Instant::now();
            for _ in 0..3 {
                let _ = guard.login(&list, name, "anythinganything");
            }
            start.elapsed()
        };
        let known = time("na");
        let unknown = time("nobody");
        assert!(
            unknown * 2 > known,
            "unknown names answer too fast: known {known:?}, unknown {unknown:?}"
        );
    }

    #[test]
    fn tokens_are_long_and_never_repeat() {
        let guard = Guard::default();
        let list = accounts();
        let first = guard.login(&list, "na", "longpassword12").unwrap();
        let second = guard.login(&list, "na", "longpassword12").unwrap();
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(first, second);
        assert!(!guard.sessions.lock().unwrap().contains_key(&first));
        assert!(guard.whose(&first).is_some() && guard.whose(&second).is_some());
    }

    #[test]
    fn locks_out_one_account_at_a_time() {
        let guard = Guard::default();
        let list = accounts();
        for _ in 0..MAX_TRIES {
            assert!(guard.login(&list, "na", "nope").is_err());
        }
        let error = guard.login(&list, "na", "longpassword12").unwrap_err();
        assert!(error.contains("Too many failed attempts"), "should be locked: {error}");
        assert!(guard.login(&list, "friend", "friendpassword").is_ok());
    }

    #[test]
    fn locks_out_an_address_not_the_owner() {
        let guard = Guard::default();
        let list = accounts();
        let stranger: std::net::IpAddr = "203.0.113.7".parse().unwrap();
        let mut refused = false;
        // Guessing across names, as an attacker would.
        for one in 0..IP_TRIES + 2 {
            let error = guard.login_from(&list, &format!("guess{one}"), "nope", Some(stranger)).unwrap_err();
            refused |= error.contains("from this address");
        }
        assert!(refused, "the address should be locked out");
        let locked = guard.login_from(&list, "friend", "friendpassword", Some(stranger)).unwrap_err();
        assert!(locked.contains("from this address"), "{locked}");
        // The owner, from this PC, still gets in.
        assert!(guard.login_from(&list, "friend", "friendpassword", Some("127.0.0.1".parse().unwrap())).is_ok());
    }

    #[test]
    #[ignore]
    fn prints_a_hash_for_a_test_account() {
        let password = std::env::var("SE_TEST_PASSWORD").expect("SE_TEST_PASSWORD is required");
        println!("HASH={}", Guard::hash(&password).unwrap());
    }

    #[test]
    fn salts_every_time() {
        let one = Guard::hash("same").unwrap();
        assert!(!one.contains("same"), "the password is stored as is");
        assert_ne!(one, Guard::hash("same").unwrap());
    }

    #[test]
    fn sends_everyone_out_when_accounts_change() {
        let guard = Guard::default();
        let list = accounts();
        let token = guard.login(&list, "friend", "friendpassword").unwrap();
        assert!(guard.whose(&token).is_some());
        guard.forget_all();
        assert!(guard.whose(&token).is_none());
    }
}

#[cfg(test)]
mod secret_tests {
    use super::*;

    #[test]
    fn debug_never_prints_the_hash() {
        let account = Account {
            name: "na".into(),
            hash: "$argon2id$v=19$m=19456,t=2,p=1$abc$def".into(),
            role: ADMIN.into(),
            servers: Vec::new(),
        };
        let printed = format!("{account:?}");
        assert!(!printed.contains("argon2"), "{printed}");
        assert!(printed.contains("[redacted]"));
    }
}
