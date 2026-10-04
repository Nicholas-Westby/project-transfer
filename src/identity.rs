//! This instance's TLS identity and the pairing code derived from two of them.

use crate::store::{write_atomic, write_atomic_private};
use anyhow::Context;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Clone)]
pub struct Identity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
}

impl Identity {
    pub fn load_or_create(dir: &Path) -> anyhow::Result<Identity> {
        let cert_path = dir.join("cert.der");
        let key_path = dir.join("key.der");
        if cert_path.exists() && key_path.exists() {
            return Ok(Identity {
                cert_der: std::fs::read(&cert_path)
                    .with_context(|| format!("could not read {}", cert_path.display()))?,
                key_der: std::fs::read(&key_path)
                    .with_context(|| format!("could not read {}", key_path.display()))?,
            });
        }
        std::fs::create_dir_all(dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::new(vec!["project-transfer".to_string()])?;
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, "project-transfer");
        params.distinguished_name = dn;
        let cert = params.self_signed(&key)?;
        let id = Identity {
            cert_der: cert.der().to_vec(),
            key_der: key.serialize_der(),
        };
        // Key first: a cert without its key would be unusable on the next start.
        write_atomic_private(&key_path, &id.key_der)?;
        write_atomic(&cert_path, &id.cert_der)?;
        Ok(id)
    }

    pub fn fingerprint(&self) -> String {
        fingerprint_of(&self.cert_der)
    }
}

pub fn fingerprint_of(cert_der: &[u8]) -> String {
    hex(&Sha256::digest(cert_der))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// None unless `text` is lowercase or uppercase hex of exactly `len` bytes.
pub fn unhex(text: &str, len: usize) -> Option<Vec<u8>> {
    if text.len() != len * 2 || !text.is_ascii() {
        return None;
    }
    (0..len)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

/// Bytes of fresh randomness each side adds to a pairing.
pub const NONCE_LEN: usize = 32;

pub fn nonce() -> [u8; NONCE_LEN] {
    rand::random()
}

/// What the initiator commits to before it sees the responder's nonce.
pub fn commitment(nonce: &[u8]) -> String {
    hex(&Sha256::digest(nonce))
}

/// Both sides show the same code, so the certificates are sorted; the nonces
/// keep their initiator-then-responder order. Because the initiator commits
/// to its nonce before seeing the other one, neither side can search for
/// keys or nonces that give a chosen code.
pub fn pairing_code(a: &str, b: &str, initiator_nonce: &[u8], responder_nonce: &[u8]) -> String {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let input = format!(
        "{lo}\n{hi}\n{}\n{}",
        hex(initiator_nonce),
        hex(responder_nonce)
    );
    let digest = Sha256::digest(input.as_bytes());
    let n = u64::from_be_bytes(digest[..8].try_into().expect("sha256 is 32 bytes"));
    let six = format!("{:06}", n % 1_000_000);
    format!("{} {}", &six[..3], &six[3..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_stable_across_reload() {
        let dir = tempfile::tempdir().unwrap();
        let a = Identity::load_or_create(&dir.path().join("identity")).unwrap();
        let b = Identity::load_or_create(&dir.path().join("identity")).unwrap();
        assert_eq!(a.cert_der, b.cert_der);
        assert_eq!(a.key_der, b.key_der);
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.fingerprint().len(), 64);
    }

    #[cfg(unix)]
    #[test]
    fn private_key_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        Identity::load_or_create(dir.path()).unwrap();
        let mode = std::fs::metadata(dir.path().join("key.der"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    const NA: [u8; 2] = [1, 2];
    const NB: [u8; 2] = [3, 4];

    #[test]
    fn pairing_code_is_order_independent_in_the_certificates() {
        assert_eq!(
            pairing_code("aa", "bb", &NA, &NB),
            pairing_code("bb", "aa", &NA, &NB)
        );
    }

    #[test]
    fn nonces_keep_their_roles() {
        assert_ne!(
            pairing_code("aa", "bb", &NA, &NB),
            pairing_code("aa", "bb", &NB, &NA)
        );
    }

    #[test]
    fn different_peers_or_nonces_get_different_codes() {
        let code = pairing_code("aa", "bb", &NA, &NB);
        assert_ne!(code, pairing_code("aa", "cc", &NA, &NB));
        assert_ne!(code, pairing_code("aa", "bb", &NA, &[3, 5]));
    }

    #[test]
    fn pairing_code_follows_the_documented_input() {
        let digest = Sha256::digest(b"aa\nbb\n0102\n0304");
        let n = u64::from_be_bytes(digest[..8].try_into().unwrap()) % 1_000_000;
        let six = format!("{n:06}");
        assert_eq!(
            pairing_code("bb", "aa", &NA, &NB),
            format!("{} {}", &six[..3], &six[3..])
        );
    }

    #[test]
    fn pairing_code_format() {
        let c = pairing_code("aa", "bb", &NA, &NB);
        assert_eq!(c.len(), 7);
        assert_eq!(c.as_bytes()[3], b' ');
        assert!(c.chars().filter(|c| *c != ' ').all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn hex_round_trips_and_checks_length() {
        let n = nonce();
        assert_eq!(unhex(&hex(&n), NONCE_LEN).unwrap(), n.to_vec());
        assert_eq!(unhex("0102", 3), None);
        assert_eq!(unhex("zz", 1), None);
        assert_ne!(nonce(), n);
    }

    #[test]
    fn fingerprint_is_lowercase_sha256_hex() {
        assert_eq!(
            fingerprint_of(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
