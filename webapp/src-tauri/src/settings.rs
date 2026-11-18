//! Persistent settings for WallForge.
//!
//! The file lives beside the other app data and holds the saved accounts and
//! the install folder. Passwords are never written here: the built-in
//! account's password is decrypted from the seed in `accounts.rs`, and any
//! password the user saves is encrypted with Windows DPAPI first.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::accounts::BUILT_IN_ACCOUNTS;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub window_size: Option<WindowSize>,
    pub install_dir: Option<String>,
    pub accounts: Vec<Account>,
    pub active_account: Option<String>,
    /// DPAPI protected passwords, keyed by lowercased account name.
    pub credentials: BTreeMap<String, String>,
    /// When on, downloads try an anonymous manifest before the Steam account.
    #[serde(default)]
    pub manifest_enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub username: String,
    #[serde(default)]
    pub signed_in: bool,
}

impl Settings {
    /// Read the settings file, tolerating a byte order mark.
    pub fn load() -> Settings {
        let path = settings_path();
        let raw = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => return Settings::default(),
        };
        let cleaned = raw.trim_start_matches('\u{feff}');
        serde_json::from_str(cleaned).unwrap_or_default()
    }

    pub fn save(&self) -> bool {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return false;
            }
        }
        let body = match serde_json::to_string_pretty(self) {
            Ok(text) => text,
            Err(_) => return false,
        };
        // Write through a temporary file so a crash cannot truncate it.
        let tmp = path.with_extension("json.tmp");
        if fs::write(&tmp, body).is_err() {
            return false;
        }
        fs::rename(&tmp, &path).is_ok()
    }

    /// The account downloads use: the selected one, else the first saved one.
    pub fn active_account(&self) -> Option<String> {
        let wanted = self
            .active_account
            .as_deref()
            .map(str::to_lowercase)
            .unwrap_or_default();
        self.accounts
            .iter()
            .find(|account| account.username.to_lowercase() == wanted)
            .or_else(|| self.accounts.first())
            .map(|account| account.username.clone())
    }

    pub fn save_account(&mut self, username: &str) {
        let username = username.trim();
        if username.is_empty() {
            return;
        }
        if !self
            .accounts
            .iter()
            .any(|account| account.username.eq_ignore_ascii_case(username))
        {
            self.accounts.push(Account {
                username: username.to_string(),
                signed_in: false,
            });
        }
    }

    pub fn set_active_account(&mut self, username: &str) -> bool {
        let mut found = None;
        for account in &self.accounts {
            if account.username.eq_ignore_ascii_case(username) {
                found = Some(account.username.clone());
                break;
            }
        }
        match found {
            Some(name) => {
                self.active_account = Some(name);
                true
            }
            None => false,
        }
    }

    pub fn remove_account(&mut self, username: &str) -> bool {
        let before = self.accounts.len();
        self.accounts
            .retain(|account| !account.username.eq_ignore_ascii_case(username));
        if self.accounts.len() == before {
            return false;
        }
        if self
            .active_account
            .as_deref()
            .map(|name| name.eq_ignore_ascii_case(username))
            .unwrap_or(false)
        {
            self.active_account = None;
        }
        self.credentials.remove(&username.to_lowercase());
        true
    }

    #[allow(dead_code)]
    pub fn mark_signed_in(&mut self, username: &str) {
        for account in &mut self.accounts {
            if account.username.eq_ignore_ascii_case(username) {
                account.signed_in = true;
            }
        }
    }

    #[allow(dead_code)]
    pub fn mark_needs_sign_in(&mut self, username: &str) {
        for account in &mut self.accounts {
            if account.username.eq_ignore_ascii_case(username) {
                account.signed_in = false;
            }
        }
    }
}

/// The accounts that ship inside the app. They cannot be edited or removed.
pub fn is_built_in(username: &str) -> bool {
    BUILT_IN_ACCOUNTS
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(username))
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("APPDATA"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("USERPROFILE")
                .map(|home| PathBuf::from(home).join("AppData").join("Local"))
                .unwrap_or_else(|_| PathBuf::from("."))
        });
    base.join("WallForge")
}

pub fn settings_path() -> PathBuf {
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("USERPROFILE")
                .map(|home| PathBuf::from(home).join("AppData").join("Roaming"))
                .unwrap_or_else(|_| PathBuf::from("."))
        });
    base.join("WallForge").join("settings.json")
}
