//! Vault file format and the two key-derivation paths.
//!
//! File layout (`vault.json.enc`), all multi-byte integers little-endian:
//!
//! ```text
//! offset  size  field
//! 0       6     magic "GAPURA"
//! 6       1     format version (0x01)
//! 7       1     kdf mode (0 = keychain key, 1 = argon2id passphrase)
//! [argon2id only]
//! 8       16    salt
//! 24      4     memory cost in KiB
//! 28      4     iterations
//! 32      4     parallelism
//! 36      12    nonce
//! [keychain only]
//! 8       12    nonce
//! ...     rest  AES-256-GCM ciphertext (tag appended by the crate)
//! ```

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use zeroize::Zeroizing;

use crate::error::{GapuraError, Result};

pub const MAGIC: &[u8; 6] = b"GAPURA";
pub const FORMAT_VERSION: u8 = 1;
pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;
pub const SALT_LEN: usize = 16;

/// Conservative argon2id defaults (design doc, section 2.3): at least 64 MiB
/// of memory, three passes.
pub const DEFAULT_MEM_KIB: u32 = 65_536;
pub const DEFAULT_ITERATIONS: u32 = 3;
pub const DEFAULT_PARALLELISM: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdfMode {
    Keychain,
    Argon2id,
}

impl KdfMode {
    fn from_byte(b: u8) -> Result<Self> {
        match b {
            0 => Ok(KdfMode::Keychain),
            1 => Ok(KdfMode::Argon2id),
            _ => Err(GapuraError::VaultCorrupt(format!(
                "unknown kdf mode byte {b}"
            ))),
        }
    }

    fn to_byte(self) -> u8 {
        match self {
            KdfMode::Keychain => 0,
            KdfMode::Argon2id => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KdfParams {
    pub salt: [u8; SALT_LEN],
    pub mem_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultHeader {
    pub kdf_mode: KdfMode,
    pub params: Option<KdfParams>,
    pub nonce: [u8; NONCE_LEN],
}

impl VaultHeader {
    pub fn keychain(nonce: [u8; NONCE_LEN]) -> Self {
        Self {
            kdf_mode: KdfMode::Keychain,
            params: None,
            nonce,
        }
    }

    pub fn argon2id(params: KdfParams, nonce: [u8; NONCE_LEN]) -> Self {
        Self {
            kdf_mode: KdfMode::Argon2id,
            params: Some(params),
            nonce,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        out.extend_from_slice(MAGIC);
        out.push(FORMAT_VERSION);
        out.push(self.kdf_mode.to_byte());
        if let Some(p) = &self.params {
            out.extend_from_slice(&p.salt);
            out.extend_from_slice(&p.mem_kib.to_le_bytes());
            out.extend_from_slice(&p.iterations.to_le_bytes());
            out.extend_from_slice(&p.parallelism.to_le_bytes());
        }
        out.extend_from_slice(&self.nonce);
        out
    }

    /// Decode a header from the start of `bytes`. Returns the header and the
    /// offset at which the ciphertext begins.
    pub fn decode(bytes: &[u8]) -> Result<(Self, usize)> {
        if bytes.len() < 8 || &bytes[..6] != MAGIC {
            return Err(GapuraError::VaultCorrupt("not a gapura vault file".into()));
        }
        let version = bytes[6];
        if version != FORMAT_VERSION {
            return Err(GapuraError::VaultVersion(version));
        }
        let kdf_mode = KdfMode::from_byte(bytes[7])?;
        let mut cursor = 8usize;
        let params = if kdf_mode == KdfMode::Argon2id {
            if bytes.len() < cursor + SALT_LEN + 12 + NONCE_LEN {
                return Err(GapuraError::VaultCorrupt("truncated argon2 header".into()));
            }
            let mut salt = [0u8; SALT_LEN];
            salt.copy_from_slice(&bytes[cursor..cursor + SALT_LEN]);
            cursor += SALT_LEN;
            let mem_kib = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            let iterations = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            let parallelism = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            Some(KdfParams {
                salt,
                mem_kib,
                iterations,
                parallelism,
            })
        } else {
            None
        };
        if bytes.len() < cursor + NONCE_LEN {
            return Err(GapuraError::VaultCorrupt("truncated header".into()));
        }
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&bytes[cursor..cursor + NONCE_LEN]);
        cursor += NONCE_LEN;
        Ok((
            Self {
                kdf_mode,
                params,
                nonce,
            },
            cursor,
        ))
    }
}

/// Fill a buffer straight from the OS entropy source. The fallible API is
/// used on purpose: entropy failure must surface, not silently degrade.
fn os_fill(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf).map_err(|e| GapuraError::Crypto(format!("os entropy source failed: {e}")))
}

pub fn random_nonce() -> Result<[u8; NONCE_LEN]> {
    let mut nonce = [0u8; NONCE_LEN];
    os_fill(&mut nonce)?;
    Ok(nonce)
}

pub fn random_key() -> Result<Zeroizing<[u8; KEY_LEN]>> {
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    os_fill(key.as_mut_slice())?;
    Ok(key)
}

/// Derive a 32-byte key from a passphrase with argon2id.
pub fn derive_key(passphrase: &str, params: &KdfParams) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    let argon = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(
            params.mem_kib,
            params.iterations,
            params.parallelism,
            Some(KEY_LEN),
        )
        .map_err(|e| GapuraError::Crypto(format!("bad argon2 params: {e}")))?,
    );
    let mut out = Box::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(passphrase.as_bytes(), &params.salt, out.as_mut_slice())
        .map_err(|e| GapuraError::Crypto(format!("argon2 failed: {e}")))?;
    Ok(Zeroizing::new(*out))
}

pub fn random_salt() -> Result<[u8; SALT_LEN]> {
    let mut salt = [0u8; SALT_LEN];
    os_fill(&mut salt)?;
    Ok(salt)
}

/// AES-256-GCM encrypt; the authentication tag is appended to the output.
pub fn encrypt(key: &[u8; KEY_LEN], nonce: &[u8; NONCE_LEN], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|e| GapuraError::Crypto(format!("bad key: {e}")))?;
    let nonce = Nonce::try_from(&nonce[..]).expect("nonce length is fixed at 12 bytes");
    cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: MAGIC.as_slice(),
            },
        )
        .map_err(|e| GapuraError::Crypto(format!("encrypt failed: {e}")))
}

/// AES-256-GCM decrypt. Any authentication failure becomes
/// [`GapuraError::WrongPassphrase`] because that is by far the most likely
/// cause from the user's point of view.
pub fn decrypt(key: &[u8; KEY_LEN], nonce: &[u8; NONCE_LEN], ciphertext: &[u8]) -> Result<Vec<u8>> {
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|e| GapuraError::Crypto(format!("bad key: {e}")))?;
    let nonce = Nonce::try_from(&nonce[..]).expect("nonce length is fixed at 12 bytes");
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad: MAGIC.as_slice(),
            },
        )
        .map_err(|_| GapuraError::WrongPassphrase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip_both_modes() {
        let nonce = random_nonce().unwrap();
        let h1 = VaultHeader::keychain(nonce);
        let (back, off) = VaultHeader::decode(&h1.encode()).unwrap();
        assert_eq!(back, h1);
        assert_eq!(off, 8 + NONCE_LEN);

        let params = KdfParams {
            salt: random_salt().unwrap(),
            mem_kib: DEFAULT_MEM_KIB,
            iterations: DEFAULT_ITERATIONS,
            parallelism: DEFAULT_PARALLELISM,
        };
        let h2 = VaultHeader::argon2id(params, nonce);
        let (back, off) = VaultHeader::decode(&h2.encode()).unwrap();
        assert_eq!(back, h2);
        assert_eq!(off, 8 + 16 + 12 + NONCE_LEN);
    }

    #[test]
    fn header_rejects_garbage() {
        assert!(VaultHeader::decode(b"not a vault at all").is_err());
        let mut bytes = VaultHeader::keychain(random_nonce().unwrap()).encode();
        bytes[6] = 9; // unknown format version
        match VaultHeader::decode(&bytes) {
            Err(GapuraError::VaultVersion(9)) => {}
            other => panic!("expected VaultVersion(9), got {other:?}"),
        }
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = random_key().unwrap();
        let nonce = random_nonce().unwrap();
        let ct = encrypt(&key, &nonce, b"hello gapura").unwrap();
        let pt = decrypt(&key, &nonce, &ct).unwrap();
        assert_eq!(pt, b"hello gapura");
    }

    #[test]
    fn decrypt_with_wrong_key_reports_wrong_passphrase() {
        let key = random_key().unwrap();
        let wrong = random_key().unwrap();
        let nonce = random_nonce().unwrap();
        let ct = encrypt(&key, &nonce, b"secret payload").unwrap();
        match decrypt(&wrong, &nonce, &ct) {
            Err(GapuraError::WrongPassphrase) => {}
            other => panic!("expected WrongPassphrase, got {other:?}"),
        }
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let key = random_key().unwrap();
        let nonce = random_nonce().unwrap();
        let mut ct = encrypt(&key, &nonce, b"payload").unwrap();
        let last = ct.len() - 1;
        ct[last] ^= 0x01;
        assert!(matches!(
            decrypt(&key, &nonce, &ct),
            Err(GapuraError::WrongPassphrase)
        ));
    }

    #[test]
    fn argon2_derivation_is_deterministic_per_salt() {
        let params = KdfParams {
            salt: [7u8; SALT_LEN],
            // Shrink the cost in tests: the defaults take seconds.
            mem_kib: 8 * 1024,
            iterations: 2,
            parallelism: 2,
        };
        let a = derive_key("correct horse", &params).unwrap();
        let b = derive_key("correct horse", &params).unwrap();
        assert_eq!(*a, *b);
        let c = derive_key("wrong horse", &params).unwrap();
        assert_ne!(*a, *c);
    }
}
