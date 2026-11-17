//! The shared Steam account that ships inside the app.
//!
//! The password is encrypted with AES-256-GCM. The key ships with the app,
//! so this keeps the password out of plain sight in the source rather than
//! hiding it from someone who unpacks the binary.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use crate::settings::Settings;

/// AES-256 key, base64 encoded.
const SHARED_ACCOUNT_KEY: &str = "MFV6n8TpDjNYfaLH7BE2W4Clyu8UOV6DqM3yFzxhhqs=";

/// (account, encrypted password)
pub const BUILT_IN_ACCOUNTS: &[(&str, &str)] = &[(
    "827743929",
    "wQPrbrdz8yIwDK+p/qvFJiMJ74Dx40sdX/6muFt+VwTq1qhQlUIYkjKH9g==",
)];

/// Decrypt one embedded account password.
pub fn decrypt_shared_account(blob: &str) -> Option<String> {
    let key = BASE64.decode(SHARED_ACCOUNT_KEY).ok()?;
    let data = BASE64.decode(blob).ok()?;
    if data.len() < 12 {
        return None;
    }
    let (nonce_bytes, body) = data.split_at(12);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let plain = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), body.as_ref())
        .ok()?;
    String::from_utf8(plain).ok()
}

/// Arm the built-in accounts: seed their names and store their password
/// encrypted for this machine.
pub fn import_shared_accounts(settings: &mut Settings) -> usize {
    let mut armed = 0;
    for (username, blob) in BUILT_IN_ACCOUNTS {
        settings.save_account(username);
        if settings.credentials.contains_key(&username.to_lowercase()) {
            // Already armed, and the user may have updated the password.
            continue;
        }
        if let Some(password) = decrypt_shared_account(blob) {
            if crate::windows_crypto::protect_secret(&password).is_ok() {
                settings
                    .credentials
                    .insert(username.to_lowercase(), crate::windows_crypto::protect_secret(&password).unwrap_or_default());
                armed += 1;
            }
        }
    }
    armed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_decrypts() {
        let (username, blob) = BUILT_IN_ACCOUNTS[0];
        let password = decrypt_shared_account(blob).expect("seed must decrypt");
        assert!(!password.is_empty());
        assert!(!username.is_empty());
    }

    #[test]
    fn broken_seed_returns_none() {
        assert!(decrypt_shared_account("not-base64!!").is_none());
        assert!(decrypt_shared_account("").is_none());
    }
}
