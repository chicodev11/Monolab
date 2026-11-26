//! Small pieces the account server shares: roles, ids, handles, tokens and rate limits.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

pub const DAY: u64 = 86_400;

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|one| one.as_secs()).unwrap_or(0)
}

/// A role in a team. Higher rank may do more.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Viewer,
    Developer,
    Admin,
    Owner,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Developer => "developer",
            Role::Admin => "admin",
            Role::Owner => "owner",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "viewer" => Role::Viewer,
            "developer" => Role::Developer,
            "admin" => Role::Admin,
            "owner" => Role::Owner,
            _ => return None,
        })
    }

    /// Owners and admins run the team: invites, roles, removing people.
    pub fn manages(self) -> bool {
        self >= Role::Admin
    }
}

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn crockford(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(CROCKFORD[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(CROCKFORD[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

pub fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("system random number generator");
    bytes
}

/// A sortable id like a ULID: 48 bits of milliseconds then 80 random bits, 26 letters.
pub fn new_id(now_secs: u64) -> String {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).map(|one| one.as_millis() as u64).unwrap_or(now_secs * 1000);
    let mut bytes = [0u8; 16];
    bytes[..6].copy_from_slice(&millis.to_be_bytes()[2..]);
    bytes[6..].copy_from_slice(&random::<10>());
    crockford(&bytes)
}

/// An invite code: 128 random bits, 26 letters (lowercase so links look calm).
pub fn new_invite_code() -> String {
    crockford(&random::<16>()).to_ascii_lowercase()
}

/// Accepts an invite code typed or pasted in any case, with the confusable letters Crockford allows.
pub fn normalize_code(code: &str) -> Option<String> {
    let mut out = String::with_capacity(26);
    for letter in code.trim().chars() {
        let letter = match letter.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            '-' => continue,
            other => other,
        };
        if !CROCKFORD.contains(&(letter as u8)) {
            return None;
        }
        out.push(letter.to_ascii_lowercase());
    }
    (out.len() == 26).then_some(out)
}

/// A secret handed to a client once: 32 random bytes, hex.
pub fn new_secret() -> String {
    hex(&random::<32>())
}

pub fn sha256(text: &str) -> Vec<u8> {
    Sha256::digest(text.as_bytes()).to_vec()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok()).collect()
}

/// Names nobody may take: they'd look official.
const RESERVED: &[&str] = &[
    "admin",
    "administrator",
    "root",
    "system",
    "support",
    "help",
    "monolab",
    "mnlab",
    "monolink",
    "official",
    "staff",
    "mod",
    "moderator",
    "owner",
    "null",
    "undefined",
    "anonymous",
    "everyone",
    "here",
    "server",
    "console",
    "api",
    "www",
    "invite",
];

/// Checks a handle and gives it back lowercase, or why it can't be used.
pub fn check_handle(text: &str) -> Result<String, &'static str> {
    let handle = text.trim().trim_start_matches('@').to_ascii_lowercase();
    if handle.len() < 3 {
        return Err("too_short");
    }
    if handle.len() > 20 {
        return Err("too_long");
    }
    if !handle.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_') {
        return Err("bad_letters");
    }
    if RESERVED.contains(&handle.as_str()) {
        return Err("reserved");
    }
    Ok(handle)
}

/// Turns a Discord name into a handle to suggest: allowed letters only, padded or cut to fit.
pub fn suggest_handle(name: &str) -> String {
    let mut handle: String = name
        .chars()
        .filter_map(|letter| {
            let letter = letter.to_ascii_lowercase();
            (letter.is_ascii_lowercase() || letter.is_ascii_digit() || letter == '_').then_some(letter)
        })
        .take(20)
        .collect();
    while handle.len() < 3 {
        handle.push('_');
    }
    handle
}

/// Counts events per key in fixed windows.
#[derive(Default)]
pub struct Limiter(Mutex<HashMap<String, (u64, u32)>>);

impl Limiter {
    /// Records one event for `key`; false when it went over `most` in this `window` (seconds).
    pub fn allow(&self, key: &str, most: u32, window: u64) -> bool {
        let now = now();
        let mut map = self.0.lock().expect("limiter poisoned");
        if map.len() > 50_000 {
            map.retain(|_, (start, _)| now < *start + window);
        }
        let entry = map.entry(key.to_string()).or_insert((now, 0));
        if now >= entry.0 + window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= most
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles() {
        assert_eq!(check_handle("@Chico_11"), Ok("chico_11".into()));
        assert_eq!(check_handle("ab"), Err("too_short"));
        assert_eq!(check_handle("a".repeat(21).as_str()), Err("too_long"));
        assert_eq!(check_handle("\u{D55C}\u{AE00}\u{C774}\u{B984}"), Err("bad_letters"));
        assert_eq!(check_handle("chi-co"), Err("bad_letters"));
        assert_eq!(check_handle("Admin"), Err("reserved"));
        assert_eq!(suggest_handle("Chico.Dev \u{D55C}"), "chicodev");
        assert_eq!(suggest_handle("\u{AC00}"), "___");
    }

    #[test]
    fn codes_and_ids() {
        let code = new_invite_code();
        assert_eq!(code.len(), 26);
        assert_eq!(normalize_code(&code.to_uppercase()).as_deref(), Some(code.as_str()));
        assert_eq!(normalize_code("short"), None);
        assert_eq!(new_id(0).len(), 26);
        assert_eq!(unhex(&hex(&[1, 2, 255])), Some(vec![1, 2, 255]));
        assert!(Role::Owner > Role::Admin && Role::Admin.manages() && !Role::Developer.manages());
    }
}
