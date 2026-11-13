//! SSH key management (PRD F8): generate ed25519 keys, inspect key files.
//!
//! v0.1 never copies private key material into the vault — the vault only
//! remembers the path, the public line, and the fingerprint. Generated keys
//! get `0600` permissions on Unix automatically.

use std::io::Write;
use std::path::Path;

use rand_core::{CryptoRng, UnwrapErr};
use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey};

use crate::error::{GapuraError, Result};

/// Info returned after inspecting or creating a key file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyFileInfo {
    /// Full OpenSSH public key line (`ssh-ed25519 AAAA... comment`).
    pub public_key: String,
    /// `SHA256:...` fingerprint, OpenSSH style.
    pub fingerprint: String,
    pub has_passphrase: bool,
}

fn os_rng() -> impl CryptoRng {
    // UnwrapErr turns the fallible OS entropy source into the infallible
    // view `PrivateKey::random` wants; a failing entropy source is a hard
    // stop, which the panic inside UnwrapErr makes explicit.
    UnwrapErr(getrandom::SysRng)
}

/// Generate a new ed25519 key pair, write `<path>` (private) and `<path>.pub`
/// (public). Refuses to overwrite existing files.
pub fn generate_ed25519(
    path: &Path,
    comment: &str,
    passphrase: Option<&str>,
) -> Result<KeyFileInfo> {
    if path.exists() {
        return Err(GapuraError::Other(format!(
            "{} already exists; pick another name to avoid clobbering keys",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| GapuraError::PathIo {
                path: parent.to_path_buf(),
                message: e.to_string(),
            })?;
        }
    }

    let mut private = PrivateKey::random(&mut os_rng(), Algorithm::Ed25519)
        .map_err(|e| GapuraError::Crypto(format!("key generation failed: {e}")))?;
    private.set_comment(comment.to_string());

    // Read the public identity from the unencrypted key: ssh-key's encrypt()
    // does not preserve the comment, and the public line is what the vault
    // stores.
    let info = KeyFileInfo {
        public_key: private
            .public_key()
            .to_openssh()
            .map_err(|e| GapuraError::Crypto(format!("public key encoding failed: {e}")))?,
        fingerprint: private
            .public_key()
            .fingerprint(HashAlg::Sha256)
            .to_string(),
        has_passphrase: passphrase.is_some(),
    };

    let private_pem = match passphrase {
        Some(p) => private
            .encrypt(&mut getrandom::SysRng, p)
            .map_err(|e| GapuraError::Crypto(format!("key encryption failed: {e}")))?
            .to_openssh(LineEnding::LF),
        None => private.to_openssh(LineEnding::LF),
    }
    .map_err(|e| GapuraError::Crypto(format!("private key encoding failed: {e}")))?;

    write_private_file(path, private_pem.as_str())?;
    write_public_file(path, format!("{}\n", info.public_key).as_str())?;
    tracing::info!(path = %path.display(), "generated ed25519 key");
    Ok(info)
}

/// Read an existing OpenSSH private key file to register it in the vault.
/// Pass the passphrase when the file is encrypted; wrong passphrases surface
/// as [`GapuraError::WrongPassphrase`].
pub fn inspect_private_key(path: &Path, passphrase: Option<&str>) -> Result<KeyFileInfo> {
    let text = std::fs::read_to_string(path).map_err(|e| GapuraError::PathIo {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let private = PrivateKey::from_openssh(text.as_str()).map_err(|e| {
        GapuraError::Other(format!(
            "{} is not a readable OpenSSH private key ({e})",
            path.display()
        ))
    })?;
    let encrypted = private.is_encrypted();
    let private = if encrypted {
        let pass = passphrase.ok_or(GapuraError::VaultLocked)?;
        private
            .decrypt(pass)
            .map_err(|_| GapuraError::WrongPassphrase)?
    } else {
        private
    };
    Ok(KeyFileInfo {
        public_key: private
            .public_key()
            .to_openssh()
            .map_err(|e| GapuraError::Crypto(format!("public key encoding failed: {e}")))?,
        fingerprint: private
            .public_key()
            .fingerprint(HashAlg::Sha256)
            .to_string(),
        has_passphrase: encrypted,
    })
}

fn write_private_file(path: &Path, contents: &str) -> Result<()> {
    let mut file = std::fs::File::create(path).map_err(|e| GapuraError::PathIo {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    file.write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| GapuraError::PathIo {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
            GapuraError::PathIo {
                path: path.to_path_buf(),
                message: e.to_string(),
            }
        })?;
    }
    Ok(())
}

fn write_public_file(path: &Path, contents: &str) -> Result<()> {
    let pub_path = public_path_for(path);
    std::fs::write(&pub_path, contents).map_err(|e| GapuraError::PathIo {
        path: pub_path,
        message: e.to_string(),
    })
}

fn public_path_for(private_path: &Path) -> std::path::PathBuf {
    let mut s = private_path.as_os_str().to_os_string();
    s.push(".pub");
    std::path::PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(dir: &Path, name: &str) -> std::path::PathBuf {
        dir.join(name)
    }

    #[test]
    fn generate_and_inspect_unencrypted() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp_path(tmp.path(), "id_gapura_test");

        let info = generate_ed25519(&path, "gapura@test", None).unwrap();
        assert!(path.is_file());
        assert!(tmp_path(tmp.path(), "id_gapura_test.pub").is_file());
        assert!(!info.has_passphrase);
        assert!(info.fingerprint.starts_with("SHA256:"));
        assert!(info.public_key.starts_with("ssh-ed25519 "));
        assert!(info.public_key.ends_with("gapura@test"));

        let back = inspect_private_key(&path, None).unwrap();
        assert_eq!(back, info);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "private key must be 0600");
        }
    }

    #[test]
    fn generate_encrypted_and_require_passphrase() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp_path(tmp.path(), "id_gapura_enc");

        let info = generate_ed25519(&path, "gapura@test", Some("keypass123")).unwrap();
        assert!(info.has_passphrase);

        // Without the passphrase the file cannot be opened.
        assert!(matches!(
            inspect_private_key(&path, None),
            Err(GapuraError::VaultLocked)
        ));
        // Wrong passphrase is reported honestly.
        assert!(matches!(
            inspect_private_key(&path, Some("wrong")),
            Err(GapuraError::WrongPassphrase)
        ));
        let back = inspect_private_key(&path, Some("keypass123")).unwrap();
        assert!(back.has_passphrase);
        assert_eq!(back.fingerprint, info.fingerprint);
    }

    #[test]
    fn refuses_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp_path(tmp.path(), "dup");
        generate_ed25519(&path, "a", None).unwrap();
        let err = generate_ed25519(&path, "b", None).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn fingerprints_are_stable() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp_path(tmp.path(), "stable");
        let a = generate_ed25519(&path, "x", None).unwrap();
        let b = inspect_private_key(&path, None).unwrap();
        assert_eq!(a.fingerprint, b.fingerprint);
    }
}
