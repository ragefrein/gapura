//! Diagnoses the OS keychain path the app uses, without the UI.
//! Run: cargo run -p gapura-core --example keyring_probe

use gapura_core::vault::keychain::{KeyStore, KeychainEntryRef, OsKeychain};

fn main() {
    let store = OsKeychain::new(KeychainEntryRef::vault_master());
    match store.load() {
        Ok(key) => println!("key found, {} bytes", key.len()),
        Err(e) => {
            println!("load failed: {e}");
            println!("code: {}", e.code());
        }
    }
}
