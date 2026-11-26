//! HTTPS for the web panel: a self-signed certificate this app makes when the host turns HTTPS on.
//! Browsers warn once per device; the app shows the fingerprint so the host can compare it.

use crate::data_root::MonolabDirs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

/// iOS refuses certificates that last longer than this.
const LIFETIME_DAYS: i64 = 397;
/// Made again once fewer days than this are left.
const RENEW_DAYS: i64 = 30;

fn folder(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("web-tls");
    std::fs::create_dir_all(&dir).map_err(|error| format!("Could not create the certificate folder: {error}"))?;
    Ok(dir)
}

fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// The names the certificate covers: this PC by name, loopback and every LAN address it has now.
pub fn names() -> Vec<String> {
    let mut names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    if let Ok(computer) = std::env::var("COMPUTERNAME") {
        let computer = computer.trim().to_ascii_lowercase();
        if !computer.is_empty() {
            names.push(computer);
        }
    }
    for choice in super::bind_choices() {
        if !names.contains(&choice.address) {
            names.push(choice.address);
        }
    }
    names
}

fn make(dir: &Path) -> Result<(), String> {
    let mut params = rcgen::CertificateParams::new(names()).map_err(|error| format!("Could not make the certificate: {error}"))?;
    params.distinguished_name.push(rcgen::DnType::CommonName, "Monolab web panel");
    let today = time::OffsetDateTime::now_utc();
    params.not_before = today - time::Duration::days(1);
    params.not_after = today + time::Duration::days(LIFETIME_DAYS);
    let key = rcgen::KeyPair::generate().map_err(|error| format!("Could not make the certificate key: {error}"))?;
    let cert = params.self_signed(&key).map_err(|error| format!("Could not make the certificate: {error}"))?;
    let write = |name: &str, bytes: &[u8]| {
        std::fs::write(dir.join(name), bytes).map_err(|error| format!("Could not save the certificate: {error}"))
    };
    write("key.der", &key.serialize_der())?;
    write("cert.der", cert.der())?;
    write("made.txt", now().to_string().as_bytes())
}

fn fresh(dir: &Path) -> bool {
    let made: i64 = std::fs::read_to_string(dir.join("made.txt")).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(0);
    dir.join("cert.der").is_file() && dir.join("key.der").is_file() && now() - made < (LIFETIME_DAYS - RENEW_DAYS) * 86_400
}

/// Makes the certificate if there is none or it runs out within a month. Called when HTTPS is turned on
/// and when the panel starts with HTTPS on.
pub fn ensure(app: &AppHandle) -> Result<(), String> {
    let dir = folder(app)?;
    if fresh(&dir) {
        return Ok(());
    }
    make(&dir)
}

/// The TLS settings for the listener, from the saved certificate.
pub fn server_config(app: &AppHandle) -> Result<Arc<rustls::ServerConfig>, String> {
    let dir = folder(app)?;
    let cert = std::fs::read(dir.join("cert.der")).map_err(|error| format!("Could not read the certificate: {error}"))?;
    let key = std::fs::read(dir.join("key.der")).map_err(|error| format!("Could not read the certificate key: {error}"))?;
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| error.to_string())?
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(cert)], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)))
        .map_err(|error| format!("The saved certificate can't be used: {error}"))?;
    Ok(Arc::new(config))
}

/// SHA-256 of the certificate, as browsers show it (AB:CD:…). Empty when there is none yet.
pub fn fingerprint(app: &AppHandle) -> String {
    let Ok(dir) = folder(app) else { return String::new() };
    std::fs::read(dir.join("cert.der")).map(|der| colon_hex(&Sha256::digest(der))).unwrap_or_default()
}

fn colon_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect::<Vec<_>>().join(":")
}

/// `Host` header → the https address of the same path, for plain requests while HTTPS is on.
pub fn https_location(host: &str, target: &str, port: u16) -> String {
    let host = host.trim();
    let host = if host.is_empty() { format!("127.0.0.1:{port}") } else { host.to_string() };
    let target = if target.starts_with('/') { target } else { "/" };
    format!("https://{host}{target}")
}

/// Where to reach a listener bound to `bound` from this PC (every network → loopback).
pub fn knock_address(bound: IpAddr) -> IpAddr {
    if bound.is_unspecified() {
        IpAddr::from([127, 0, 0, 1])
    } else {
        bound
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_reads_like_a_browser() {
        assert_eq!(colon_hex(&[0xab, 0x01, 0xff]), "AB:01:FF");
    }

    #[test]
    fn plain_requests_move_to_https() {
        assert_eq!(https_location("192.168.0.5:25566", "/a?b=1", 25566), "https://192.168.0.5:25566/a?b=1");
        assert_eq!(https_location("", "/", 25566), "https://127.0.0.1:25566/");
        assert_eq!(https_location("x", "http://evil/", 1), "https://x/");
    }

    #[test]
    fn makes_a_certificate_rustls_accepts() {
        let dir = std::env::temp_dir().join(format!("monolab-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        make(&dir).unwrap();
        assert!(fresh(&dir));
        let cert = std::fs::read(dir.join("cert.der")).unwrap();
        let key = std::fs::read(dir.join("key.der")).unwrap();
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(cert)], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)))
            .expect("rustls should take the certificate");
        std::fs::write(dir.join("made.txt"), "0").unwrap();
        assert!(!fresh(&dir), "an old certificate is made again");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
