//! Database certificates: the app looks at the certificate a MySQL/MariaDB server shows before sending a
//! password, so a database with a self-signed certificate can be trusted on first use, the way SSH does.
//! mysql_async takes no custom verifier, so this opens its own short connection: the MySQL greeting, an
//! SSL request, and a TLS handshake that only records the certificate.

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::Digest;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const CLIENT_LONG_PASSWORD: u32 = 0x1;
const CLIENT_PROTOCOL_41: u32 = 0x200;
const CLIENT_SSL: u32 = 0x800;
const CLIENT_SECURE_CONNECTION: u32 = 0x8000;

/// What the database said when the app asked for its certificate.
#[derive(Debug, PartialEq)]
pub enum Peek {
    Certificate(String),
    NoEncryption,
}

#[derive(Debug)]
struct Recorder {
    seen: Mutex<Option<Vec<u8>>>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for Recorder {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        *self.seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(end_entity.to_vec());
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// SHA-256 of a certificate, as `AB:CD:…`.
pub fn fingerprint(der: &[u8]) -> String {
    sha2::Sha256::digest(der).iter().map(|byte| format!("{byte:02X}")).collect::<Vec<_>>().join(":")
}

fn read_packet(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut head = [0_u8; 4];
    stream.read_exact(&mut head)?;
    let length = u32::from_le_bytes([head[0], head[1], head[2], 0]) as usize;
    let mut body = vec![0_u8; length];
    stream.read_exact(&mut body)?;
    Ok(body)
}

/// The capability flags in a MySQL protocol 10 greeting, or None for an error packet or something else.
fn greeting_flags(body: &[u8]) -> Option<u32> {
    if body.first() != Some(&10) {
        return None;
    }
    let end_of_version = body.iter().skip(1).position(|byte| *byte == 0)? + 1;
    // version NUL, connection id (4), auth data part 1 (8), filler (1)
    let low = end_of_version + 1 + 4 + 8 + 1;
    let lower = u16::from_le_bytes([*body.get(low)?, *body.get(low + 1)?]) as u32;
    // charset (1), status (2), then the upper flags, which very old servers leave out
    let high = low + 2 + 1 + 2;
    let upper = match (body.get(high), body.get(high + 1)) {
        (Some(first), Some(second)) => u16::from_le_bytes([*first, *second]) as u32,
        _ => 0,
    };
    Some(lower | (upper << 16))
}

/// The SSL request packet (sequence 1) a client sends to switch to TLS before logging in.
fn ssl_request() -> Vec<u8> {
    let flags = CLIENT_LONG_PASSWORD | CLIENT_PROTOCOL_41 | CLIENT_SSL | CLIENT_SECURE_CONNECTION;
    let mut out = vec![32, 0, 0, 1];
    out.extend(flags.to_le_bytes());
    out.extend(16_777_216_u32.to_le_bytes());
    out.push(45); // utf8mb4_general_ci
    out.extend([0_u8; 23]);
    out
}

/// Connects far enough to see the database's certificate and returns its fingerprint. Sends no login.
pub fn peek(address: &str, port: u16, wait: Duration) -> Result<Peek, String> {
    let unreachable = "Could not connect to the database. Check the address, port and firewall.";
    let target = (address, port).to_socket_addrs().map_err(|_| unreachable.to_string())?.next().ok_or(unreachable)?;
    let mut stream = TcpStream::connect_timeout(&target, wait).map_err(|_| unreachable.to_string())?;
    let _ = stream.set_read_timeout(Some(wait));
    let _ = stream.set_write_timeout(Some(wait));
    let greeting = read_packet(&mut stream).map_err(|_| unreachable.to_string())?;
    let flags = greeting_flags(&greeting).ok_or("This address did not answer like a MySQL or MariaDB database.")?;
    if flags & CLIENT_SSL == 0 {
        return Ok(Peek::NoEncryption);
    }
    stream.write_all(&ssl_request()).map_err(|_| unreachable.to_string())?;
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let recorder = Arc::new(Recorder { seen: Mutex::new(None), provider: provider.clone() });
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| error.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(recorder.clone())
        .with_no_client_auth();
    let name = ServerName::try_from(address.to_string()).unwrap_or_else(|_| ServerName::try_from("localhost").expect("a fixed name"));
    let mut tls = rustls::ClientConnection::new(Arc::new(config), name).map_err(|error| error.to_string())?;
    let taken = || recorder.seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    loop {
        if let Some(der) = taken() {
            return Ok(Peek::Certificate(fingerprint(&der)));
        }
        if !tls.is_handshaking() {
            return Err("The database did not show a certificate.".into());
        }
        tls.complete_io(&mut stream).map_err(|error| format!("Could not read the database's certificate: {error}"))?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn greeting(flags: u32) -> Vec<u8> {
        let mut body = vec![10];
        body.extend(b"8.0.36\0");
        body.extend([1, 0, 0, 0]);
        body.extend([b'a'; 8]);
        body.push(0);
        body.extend(((flags & 0xffff) as u16).to_le_bytes());
        body.push(45);
        body.extend([2, 0]);
        body.extend(((flags >> 16) as u16).to_le_bytes());
        body
    }

    #[test]
    fn reads_the_greeting_flags() {
        let flags = CLIENT_PROTOCOL_41 | CLIENT_SSL | 0x0008_0000;
        assert_eq!(greeting_flags(&greeting(flags)), Some(flags));
        assert_eq!(greeting_flags(&[0xff, 0x10, 0x04]), None);
        assert_eq!(greeting_flags(&[10, b'5']), None);
    }

    #[test]
    fn the_ssl_request_is_one_32_byte_packet() {
        let packet = ssl_request();
        assert_eq!(packet.len(), 36);
        assert_eq!(&packet[..4], &[32, 0, 0, 1]);
        let flags = u32::from_le_bytes([packet[4], packet[5], packet[6], packet[7]]);
        assert!(flags & CLIENT_SSL != 0 && flags & CLIENT_PROTOCOL_41 != 0);
    }

    #[test]
    fn fingerprints_are_sha256_in_pairs() {
        let print = fingerprint(b"certificate");
        assert_eq!(print.len(), 32 * 3 - 1);
        assert!(print.split(':').all(|pair| pair.len() == 2));
    }
}
