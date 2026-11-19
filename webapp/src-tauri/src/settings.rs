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
    /// The manual install folder, kept with forward slashes in the file so it
    /// reads as `C:/Steam/...` instead of the escaped `C:\\Steam\\...`.
    #[serde(serialize_with = "with_forward_slashes")]
    pub install_dir: Option<String>,
    pub accounts: Vec<Account>,
    pub active_account: Option<String>,
    /// DPAPI protected passwords, keyed by lowercased account name.
    pub credentials: BTreeMap<String, String>,
    /// When on, downloads try an anonymous manifest before the Steam account.
    #[serde(default)]
    pub manifest_enabled: bool,
    /// Set when the file held a path in the old escaped form, so the next
    /// launch rewrites it with forward slashes.
    #[serde(skip)]
    pub needs_save: bool,
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
        let mut settings: Settings = serde_json::from_str(cleaned).unwrap_or_default();
        // The file stores forward slashes; the rest of the app works with the
        // native form, which the download tool and Explorer both expect.
        // Rewrite the file when it still holds the old escaped form.
        let stored = settings.install_dir.clone();
        settings.install_dir = stored.as_deref().map(native_path);
        // The stored text already has one separator per JSON value; a
        // backslash means the file was written before the forward-slash
        // change, so rewrite it once.
        settings.needs_save = stored
            .as_deref()
            .is_some_and(|value| value.contains('\\') || tidy_path(value) != value);
        settings
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

/// The one folder WallForge keeps its own files in: `%APPDATA%\WallForge`.
///
/// Cached depot manifests are the exception: they live next to the wallpaper
/// they belong to, so they are removed together with it.
pub fn data_dir() -> PathBuf {
    roaming_dir().join("WallForge")
}

fn roaming_dir() -> PathBuf {
    std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| {
        std::env::var("USERPROFILE")
            .map(|home| PathBuf::from(home).join("AppData").join("Roaming"))
            .unwrap_or_else(|_| PathBuf::from("."))
    })
}

pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

/// The WebView2 profile: filters, sort choices and the rest of the UI state.
pub fn webview_dir() -> PathBuf {
    data_dir().join("webview")
}

/// Write a path the way the settings file shows it: one forward slash per
/// separator, so a Windows `\` does not have to be escaped as `\\` in JSON.
fn with_forward_slashes<S>(value: &Option<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(path) => serializer.serialize_some(&tidy_path(path).replace('\\', "/")),
        None => serializer.serialize_none(),
    }
}

/// Tidy a path the user typed or picked: drop quotes and stray whitespace,
/// collapse repeated separators and remove a trailing one. A UNC prefix
/// (`\\server\share`) keeps its leading double slash.
pub fn tidy_path(value: &str) -> String {
    let trimmed = value.trim().trim_matches('"').trim().replace('\\', "/");
    let (prefix, rest) = match trimmed.strip_prefix("//") {
        Some(rest) => ("//", rest),
        None => ("", trimmed.as_str()),
    };
    let mut out = String::with_capacity(trimmed.len() + 1);
    out.push_str(prefix);
    let mut previous_was_slash = false;
    for ch in rest.chars() {
        if ch == '/' {
            if previous_was_slash {
                continue;
            }
            previous_was_slash = true;
        } else {
            previous_was_slash = false;
        }
        out.push(ch);
    }
    while out.len() > 1 && out.ends_with('/') && !out.ends_with(":/") {
        out.pop();
    }
    out
}

/// The native form of a stored path, which Windows APIs understand.
pub fn native_path(value: &str) -> String {
    tidy_path(value).replace('/', "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_with(install_dir: &str) -> Settings {
        Settings {
            install_dir: Some(install_dir.to_string()),
            ..Settings::default()
        }
    }

    #[test]
    fn the_install_folder_is_written_with_single_forward_slashes() {
        let json = serde_json::to_string(&settings_with(
            r"C:\Program Files (x86)\Steam\steamapps\common\Wallpaper Engine",
        ))
        .expect("settings must serialize");
        assert!(json.contains(
            r#""install_dir":"C:/Program Files (x86)/Steam/steamapps/common/Wallpaper Engine""#
        ));
        // No escaped separators anywhere in the file.
        assert!(!json.contains('\\'));
    }

    #[test]
    fn a_stored_path_comes_back_native() {
        assert_eq!(
            native_path("C:/Games//Wallpaper Engine/"),
            r"C:\Games\Wallpaper Engine"
        );
        assert_eq!(native_path("D:/Steam"), r"D:\Steam");
    }

    #[test]
    fn a_unc_prefix_keeps_its_double_slash() {
        assert_eq!(
            tidy_path(r"\\server\share\\wallpapers\\"),
            "//server/share/wallpapers"
        );
        assert_eq!(native_path("//server/share/"), r"\\server\share");
    }

    #[test]
    fn a_drive_root_keeps_its_slash() {
        assert_eq!(tidy_path(r"C:\\"), "C:/");
    }
}
