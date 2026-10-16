
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

const SESSION_LIFE: Duration = Duration::from_secs(12 * 60 * 60);

const MAX_TRIES: u32 = 8;

const LOCKOUT: Duration = Duration::from_secs(10 * 60);

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

    pub fn may(&self, server_id: &str) -> bool {
        self.is_admin() || self.servers.iter().any(|one| one == server_id)
    }
}

#[derive(Clone, Debug)]
struct Session {
    account: Account,
    until: Instant,
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

    pub fn login(&self, accounts: &[Account], name: &str, password: &str) -> Result<String, String> {
        let name = name.trim();
        {
            let mut tries = self.tries.lock().expect("auth poisoned");
            let one = tries.entry(name.to_string()).or_insert((0, None));
            if let Some(since) = one.1 {
                if since.elapsed() < LOCKOUT {
                    let left = (LOCKOUT - since.elapsed()).as_secs() / 60 + 1;
                    return Err(format!("Too many failed attempts. Try again in {left} minutes."));
                }
                *one = (0, None);
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
            return Err("Wrong name or password.".into());
        }
        self.tries.lock().expect("auth poisoned").remove(name);
        let token = super::make_key();
        self.sessions.lock().expect("auth poisoned").insert(
            digest(&token),
            Session {
                account: found.expect("Marked as matching, so it must exist").clone(),
                until: Instant::now() + SESSION_LIFE,
            },
        );
        Ok(token)
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
