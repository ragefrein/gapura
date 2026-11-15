//! Runs the exact vault unlock path the app uses, without the UI.
//! Run: cargo run -p gapura-core --example unlock_probe

use gapura_core::vault::{
    self,
    keychain::{KeychainEntryRef, OsKeychain},
};
use std::time::Instant;

fn main() {
    let dir = dirs::data_dir().unwrap().join("gapura");
    let store = OsKeychain::new(KeychainEntryRef::vault_master());
    let start = Instant::now();
    match vault::unlock(&dir, None, &store) {
        Ok(v) => println!(
            "unlocked in {:?}: {} connections, mode={}",
            start.elapsed(),
            v.payload.connections.len(),
            v.mode().as_str()
        ),
        Err(e) => println!(
            "unlock FAILED after {:?}: {e} (code {})",
            start.elapsed(),
            e.code()
        ),
    }
}
