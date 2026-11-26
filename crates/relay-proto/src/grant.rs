//! Monolink role grants: the account server signs "this person, on this device, is a Developer in
//! this team, for these servers on that host" and the host checks it on every iroh connection.
//! Section 5 of `docs/monolink.md`.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::DeviceId;

const CONTEXT: &[u8] = b"monolab-grant-v1";
/// How long a grant lasts. Apps fetch a new one a few minutes before.
pub const LIFE_SECS: u64 = 15 * 60;
/// How far ahead of the host's clock a grant may claim to be made.
pub const CLOCK_SLACK: u64 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub v: u8,
    /// Which of the account server's keys signed it.
    pub kid: u8,
    pub user: String,
    /// The name shown in console lines and logs.
    pub handle: String,
    /// The only device that may use this grant: the iroh peer of the connection.
    pub device: DeviceId,
    pub team: String,
    pub team_version: u64,
    pub role: Role,
    /// The host this grant is for, and its servers in the team (ids inside the host app).
    pub host: DeviceId,
    pub servers: Vec<String>,
    pub iat: u64,
    pub exp: u64,
}

/// Why a host turned a grant down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    Malformed,
    UnknownKey,
    BadSignature,
    WrongDevice,
    WrongHost,
    Expired,
    FromTheFuture,
    Outdated,
}

impl Refused {
    pub fn code(self) -> &'static str {
        match self {
            Refused::Malformed => "malformed",
            Refused::UnknownKey => "unknown_key",
            Refused::BadSignature => "bad_signature",
            Refused::WrongDevice => "wrong_device",
            Refused::WrongHost => "wrong_host",
            Refused::Expired => "expired",
            Refused::FromTheFuture => "clock",
            Refused::Outdated => "outdated",
        }
    }
}

fn message(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(CONTEXT.len() + body.len());
    out.extend_from_slice(CONTEXT);
    out.extend_from_slice(body);
    out
}

/// Signs a grant. The text form is `hex(body).hex(signature)`, easy to carry in JSON.
pub fn sign(grant: &Grant, key: &SigningKey) -> String {
    let body = rmp_serde::to_vec_named(grant).expect("grants always encode");
    let signature = key.sign(&message(&body));
    format!("{}.{}", crate::hex(&body), crate::hex(&signature.to_bytes()))
}

/// Reads a signed grant and checks its signature against the keys this app trusts. Says nothing yet
/// about whether it fits the connection; see [`Grant::check`].
pub fn open(text: &str, keys: &[(u8, [u8; 32])]) -> Result<Grant, Refused> {
    let (body, signature) = text.split_once('.').ok_or(Refused::Malformed)?;
    let body = crate::unhex(body).ok_or(Refused::Malformed)?;
    let signature: [u8; 64] = crate::unhex(signature).and_then(|one| one.try_into().ok()).ok_or(Refused::Malformed)?;
    if body.len() > 16 * 1024 {
        return Err(Refused::Malformed);
    }
    let grant: Grant = rmp_serde::from_slice(&body).map_err(|_| Refused::Malformed)?;
    if grant.v != 1 {
        return Err(Refused::Malformed);
    }
    let key = keys.iter().find(|(kid, _)| *kid == grant.kid).ok_or(Refused::UnknownKey)?;
    let key = VerifyingKey::from_bytes(&key.1).map_err(|_| Refused::UnknownKey)?;
    key.verify(&message(&body), &Signature::from_bytes(&signature)).map_err(|_| Refused::BadSignature)?;
    Ok(grant)
}

impl Grant {
    /// The host's checks (section 5.2, except "this host attached the server too", which only the host knows).
    pub fn check(&self, peer: &DeviceId, me: &DeviceId, now: u64, seen_version: u64) -> Result<(), Refused> {
        if self.device != *peer {
            return Err(Refused::WrongDevice);
        }
        if self.host != *me {
            return Err(Refused::WrongHost);
        }
        if now >= self.exp {
            return Err(Refused::Expired);
        }
        if self.iat > now + CLOCK_SLACK {
            return Err(Refused::FromTheFuture);
        }
        if self.team_version < seen_version {
            return Err(Refused::Outdated);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(key: &SigningKey) -> (Grant, DeviceId, DeviceId) {
        let guest = DeviceId([1; 16]);
        let host = DeviceId([2; 16]);
        let grant = Grant {
            v: 1,
            kid: 1,
            user: "01USER".into(),
            handle: "chico".into(),
            device: guest,
            team: "01TEAM".into(),
            team_version: 4,
            role: Role::Developer,
            host,
            servers: vec!["survival".into()],
            iat: 1000,
            exp: 1000 + LIFE_SECS,
        };
        let _ = key;
        (grant, guest, host)
    }

    #[test]
    fn signed_grants_open_and_check() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let keys = [(1u8, key.verifying_key().to_bytes())];
        let (made, guest, host) = grant(&key);
        let text = sign(&made, &key);
        let opened = open(&text, &keys).unwrap();
        assert_eq!(opened, made);
        assert_eq!(opened.check(&guest, &host, 1100, 4), Ok(()));
        assert_eq!(opened.check(&host, &host, 1100, 4), Err(Refused::WrongDevice));
        assert_eq!(opened.check(&guest, &guest, 1100, 4), Err(Refused::WrongHost));
        assert_eq!(opened.check(&guest, &host, 1000 + LIFE_SECS, 4), Err(Refused::Expired));
        assert_eq!(opened.check(&guest, &host, 900, 4), Err(Refused::FromTheFuture));
        assert_eq!(opened.check(&guest, &host, 1100, 5), Err(Refused::Outdated));
    }

    #[test]
    fn tampering_and_strangers_are_refused() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let (made, _, _) = grant(&key);
        let text = sign(&made, &key);
        let other = SigningKey::from_bytes(&[8; 32]);
        assert_eq!(open(&text, &[(1, other.verifying_key().to_bytes())]), Err(Refused::BadSignature));
        assert_eq!(open(&text, &[(2, key.verifying_key().to_bytes())]), Err(Refused::UnknownKey));
        let mut changed = made.clone();
        changed.role = Role::Owner;
        let forged = format!("{}.{}", sign(&changed, &other).split('.').next().unwrap(), text.split('.').nth(1).unwrap());
        assert_eq!(open(&forged, &[(1, key.verifying_key().to_bytes())]), Err(Refused::BadSignature));
        assert_eq!(open("nope", &[]), Err(Refused::Malformed));
    }
}
