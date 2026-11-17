//! Experimental downloads from ManifestHub manifests.
//!
//! A free ManifestHub key lives 24 hours. We persist when it was pasted and
//! derive `missing / valid / expired` from that; the API's 403 is the only
//! ground truth. Downloads can use a manifest instead of a Steam sign-in:
//!
//!   workshop id -> hcontent_file (Steam's keyless Web API)
//!   -> GET ManifestHub?apikey&depotid&manifestid
//!   -> stage in %TEMP%, keep one copy under <item>\.DepotDownloader\
//!   -> DepotDownloaderMod -ugc <gid> -depotkeys ... -manifestfile ...

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use serde::Serialize;
use serde_json::Value;

use crate::settings::Settings;
use crate::steam;

pub const TOKEN_TTL_SECS: i64 = 24 * 60 * 60;
pub const KEY_GEN_URL: &str = "https://manifesthub2.filegear-sg.me";
const MANIFEST_API: &str = "https://api.manifesthub2.filegear-sg.me/manifest";
const DEPOTKEYS_URL: &str =
    "https://raw.githubusercontent.com/SteamAutoCracks/ManifestHub/main/depotkeys.json";
const DEPOT_ID: &str = "431960";
const WANTED_DEPOT_KEYS: [&str; 3] = ["431960", "431961", "1790230"];
const KEY_FILE: &str = "431960.key";
pub const MANIFEST_MAGIC: [u8; 4] = [0xd0, 0x17, 0xf6, 0x71];

const MAX_ATTEMPTS: usize = 4;
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
];
const USER_AGENT: &str = "WallForge/1.0 (ManifestHub)";

/* ---------------- token state ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenState {
    Missing,
    Valid,
    Expired,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenStatus {
    pub token: String,
    /// Unix seconds when the key was pasted; 0 when empty.
    pub saved_at: i64,
    pub expires_at: i64,
    /// Backend clock at measurement time; the UI anchors its countdown here.
    pub measured_at: i64,
    pub remaining_secs: i64,
    pub state: TokenState,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub status: TokenStatus,
    /// Set when the pasted text does not look like a ManifestHub key.
    pub warning: Option<String>,
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// Hex, at least 32 characters. A warning only — the API has the final say.
pub fn looks_like_token(token: &str) -> bool {
    let text = token.trim();
    text.len() >= 32 && text.chars().all(|ch| ch.is_ascii_hexdigit())
}

pub fn status_from(token: String, saved_at: i64) -> TokenStatus {
    let now = now_secs();
    let has_token = !token.trim().is_empty();
    let expires_at = if has_token && saved_at > 0 {
        saved_at + TOKEN_TTL_SECS
    } else {
        0
    };
    let remaining_secs = if has_token && saved_at > 0 {
        expires_at - now
    } else {
        0
    };
    let state = if !has_token {
        TokenState::Missing
    } else if remaining_secs > 0 {
        TokenState::Valid
    } else {
        TokenState::Expired
    };
    TokenStatus {
        token,
        saved_at,
        expires_at,
        measured_at: now,
        remaining_secs,
        state,
    }
}

pub fn status(settings: &Settings) -> TokenStatus {
    status_from(settings.manifest_token.clone(), settings.manifest_token_time)
}

/// The key a download can use. Empty means missing or expired.
pub fn current_token(settings: &Settings) -> String {
    let status = status(settings);
    if status.state == TokenState::Valid {
        status.token
    } else {
        String::new()
    }
}

/* ---------------- ManifestHub HTTP ---------------- */

fn client(timeout: Duration) -> Result<Client, String> {
    Client::builder()
        .user_agent(USER_AGENT)
        .timeout(timeout)
        .build()
        .map_err(|err| err.to_string())
}

fn snippet(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(300)])
        .trim()
        .to_string()
}

/// `workshop id -> hcontent_file`, via Steam's keyless details API.
pub fn resolve_gid(client: &Client, workshop_id: &str) -> Result<String, String> {
    let form = vec![
        ("itemcount".to_string(), "1".to_string()),
        ("publishedfileids[0]".to_string(), workshop_id.to_string()),
    ];
    let response = client
        .post(steam::STEAM_API_URL)
        .header("Accept-Language", "en-US,en;q=0.9")
        .form(&form)
        .send()
        .map_err(|err| err.to_string())?;
    let payload: Value = response.json().map_err(|err| err.to_string())?;
    let entry = payload
        .pointer("/response/publishedfiledetails/0")
        .ok_or_else(|| "Steam returned no details for this item.".to_string())?;
    if entry.get("result").and_then(Value::as_i64) != Some(1) {
        return Err("That item is private, removed, or unavailable.".to_string());
    }
    let gid = entry
        .get("hcontent_file")
        .map(|value| match value {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default();
    if gid.is_empty() || gid == "0" {
        return Err("That item has no depot manifest; it may be a collection.".to_string());
    }
    Ok(gid)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl FetchError {
    fn new(code: &'static str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

enum Attempt {
    TokenRejected(String),
    Missing(String),
    Fatal(String),
    Transient(String),
}

fn unzip_manifest(raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(raw)).map_err(|err| format!("zip: {err}"))?;
    let mut entry = if archive.by_name("z").is_ok() {
        archive.by_name("z").map_err(|err| format!("zip member z: {err}"))?
    } else if !archive.is_empty() {
        archive
            .by_index(0)
            .map_err(|err| format!("zip member 0: {err}"))?
    } else {
        return Err("manifest archive is empty".to_string());
    };
    let mut inner = Vec::new();
    entry
        .read_to_end(&mut inner)
        .map_err(|err| format!("zip read: {err}"))?;
    if inner.get(..4) != Some(&MANIFEST_MAGIC) {
        return Err("manifest archive did not contain a depot manifest".to_string());
    }
    Ok(inner)
}

fn fetch_once(client: &Client, gid: &str, token: &str) -> Result<Vec<u8>, Attempt> {
    let response = client
        .get(MANIFEST_API)
        .query(&[("apikey", token), ("depotid", DEPOT_ID), ("manifestid", gid)])
        .send()
        .map_err(|err| Attempt::Transient(format!("network: {err}")))?;

    let status = response.status().as_u16();
    let body = response
        .bytes()
        .map_err(|err| Attempt::Transient(format!("network: {err}")))?;

    match status {
        403 => {
            return Err(Attempt::TokenRejected(
                "ManifestHub rejected the API key. Open Experimental to save a new one.".to_string(),
            ))
        }
        404 => {
            return Err(Attempt::Missing(format!(
                "ManifestHub has no copy of manifest {gid}."
            )))
        }
        429 => return Err(Attempt::Transient("ManifestHub rate limited us (429).".to_string())),
        500..=599 => {
            return Err(Attempt::Transient(format!(
                "HTTP {status} {}",
                snippet(&body)
            )))
        }
        other if !(200..=299).contains(&other) => {
            return Err(Attempt::Fatal(format!(
                "ManifestHub HTTP {other}: {}",
                snippet(&body)
            )))
        }
        _ => {}
    }

    if body.is_empty() {
        return Err(Attempt::Transient("ManifestHub returned an empty body.".to_string()));
    }
    if body.get(..4) == Some(&MANIFEST_MAGIC) {
        return Ok(body.to_vec());
    }
    if body.get(..2) == Some(b"PK") {
        return unzip_manifest(&body).map_err(Attempt::Fatal);
    }

    let text = snippet(&body);
    if text.starts_with('{') {
        let message = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or(text);
        if message.to_lowercase().contains("unable to download") {
            // They sometimes report this with a 200; a retry normally works.
            return Err(Attempt::Transient(message));
        }
        return Err(Attempt::Fatal(format!("ManifestHub: {message}")));
    }

    Err(Attempt::Fatal(format!(
        "unrecognised manifest payload ({} bytes)",
        body.len()
    )))
}

/// Fetch raw `.manifest` bytes, retrying only transient failures.
pub fn fetch_manifest(
    gid: &str,
    token: &str,
    log: impl Fn(&str),
) -> Result<Vec<u8>, FetchError> {
    if token.trim().is_empty() {
        return Err(FetchError::new(
            "TOKEN_REJECTED",
            "No ManifestHub key is saved. Open Experimental to add one.",
            false,
        ));
    }
    let client = client(Duration::from_secs(90))
        .map_err(|err| FetchError::new("FATAL", format!("HTTP client: {err}"), false))?;

    let mut last = String::new();
    for attempt in 0..MAX_ATTEMPTS {
        match fetch_once(&client, gid, token) {
            Ok(bytes) => return Ok(bytes),
            Err(Attempt::TokenRejected(message)) => {
                return Err(FetchError::new("TOKEN_REJECTED", message, false))
            }
            Err(Attempt::Missing(message)) => {
                return Err(FetchError::new("MANIFEST_MISSING", message, false))
            }
            Err(Attempt::Fatal(message)) => {
                return Err(FetchError::new("FATAL", message, false))
            }
            Err(Attempt::Transient(message)) => {
                last = message;
                if attempt + 1 == MAX_ATTEMPTS {
                    break;
                }
                let delay = RETRY_DELAYS[attempt.min(RETRY_DELAYS.len() - 1)];
                log(&format!(
                    "ManifestHub hiccup - retry {}/{} in {}s",
                    attempt + 2,
                    MAX_ATTEMPTS,
                    delay.as_secs()
                ));
                std::thread::sleep(delay);
            }
        }
    }
    Err(FetchError::new(
        "RETRY_EXHAUSTED",
        format!("ManifestHub failed after {MAX_ATTEMPTS} attempts: {last}"),
        true,
    ))
}

/* ---------------- depot keys and staging ---------------- */

fn key_file_path() -> PathBuf {
    crate::settings::data_dir().join(KEY_FILE)
}

fn key_file_has_depot(path: &Path, depot: &str) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    text.lines()
        .any(|line| line.trim_start().starts_with(&format!("{depot};")))
}

/// Path to a key file containing the Wallpaper Engine depot key.
/// Fetched once from ManifestHub's public list and cached in app data.
pub fn ensure_depot_keys() -> Result<PathBuf, String> {
    let path = key_file_path();
    if key_file_has_depot(&path, DEPOT_ID) {
        return Ok(path);
    }

    let client = client(Duration::from_secs(180))?;
    let response = client
        .get(DEPOTKEYS_URL)
        .send()
        .map_err(|err| format!("Could not download depot keys: {err}"))?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(format!("Could not download depot keys (HTTP {status})."));
    }
    let table: Value = response
        .json()
        .map_err(|err| format!("Depot key list was not JSON: {err}"))?;

    let mut lines = Vec::new();
    for depot in WANTED_DEPOT_KEYS {
        if let Some(key) = table.get(depot).and_then(Value::as_str) {
            if !key.trim().is_empty() {
                lines.push(format!("{depot};{}", key.trim()));
            }
        }
    }
    if !lines.iter().any(|line| line.starts_with(&format!("{DEPOT_ID};"))) {
        return Err("The depot key list did not contain Wallpaper Engine's key.".to_string());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::write(&path, format!("{}\n", lines.join("\n"))).map_err(|err| err.to_string())?;
    Ok(path)
}

pub fn manifest_name(gid: &str) -> String {
    format!("{DEPOT_ID}_{gid}.manifest")
}

pub fn cached_manifest(item_dir: &Path, gid: &str) -> Option<PathBuf> {
    let path = item_dir
        .join(".DepotDownloader")
        .join(manifest_name(gid));
    if is_manifest_file(&path) {
        Some(path)
    } else {
        None
    }
}

pub fn is_manifest_file(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.get(..4) == Some(&MANIFEST_MAGIC)
}

/// Write through %TEMP%, then keep the only permanent copy beside the item.
/// The staging file is removed even when the copy fails.
pub fn stage_manifest(
    workshop_id: &str,
    gid: &str,
    bytes: &[u8],
    item_dir: &Path,
) -> Result<PathBuf, String> {
    let temp = std::env::temp_dir().join(format!(
        "wallforge_{}_{}.manifest",
        workshop_id, gid
    ));
    let final_path = item_dir
        .join(".DepotDownloader")
        .join(manifest_name(gid));

    fs::write(&temp, bytes).map_err(|err| format!("Could not stage the manifest: {err}"))?;
    let result = (|| -> Result<(), String> {
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        fs::copy(&temp, &final_path)
            .map_err(|err| format!("Could not save the manifest: {err}"))?;
        Ok(())
    })();
    let _ = fs::remove_file(&temp);
    result?;
    Ok(final_path)
}

/// Arguments for the keyless, manifest-backed download path.
pub fn depot_args(
    exe: &Path,
    gid: &str,
    key_file: &Path,
    manifest: &Path,
    item_dir: &Path,
) -> Vec<String> {
    vec![
        exe.to_string_lossy().to_string(),
        "-app".to_string(),
        APP_ID.to_string(),
        "-ugc".to_string(),
        gid.to_string(),
        "-depotkeys".to_string(),
        key_file.to_string_lossy().to_string(),
        "-manifestfile".to_string(),
        manifest.to_string_lossy().to_string(),
        "-dir".to_string(),
        item_dir.to_string_lossy().to_string(),
        "-verify-all".to_string(),
        "-max-downloads".to_string(),
        "16".to_string(),
    ]
}

const APP_ID: &str = "431960";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_states_are_derived_from_the_saved_time() {
        let now = now_secs();
        assert_eq!(status_from(String::new(), 0).state, TokenState::Missing);
        assert_eq!(status_from("a".repeat(76), 0).state, TokenState::Expired);
        assert_eq!(
            status_from("a".repeat(76), now - TOKEN_TTL_SECS + 30).state,
            TokenState::Valid
        );
        assert_eq!(
            status_from("a".repeat(76), now - TOKEN_TTL_SECS).state,
            TokenState::Expired
        );
    }

    #[test]
    fn token_text_is_checked_but_not_trusted() {
        assert!(looks_like_token(&format!("5daf5c94{}", "0".repeat(68))));
        assert!(looks_like_token(&"a".repeat(76)));
        assert!(!looks_like_token("not-hex-at-all"));
    }

    #[test]
    fn the_magic_identifies_a_manifest() {
        let dir = std::env::temp_dir().join(format!("wallforge-manifest-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.manifest");
        assert!(!is_manifest_file(&path));
        fs::write(&path, [0xd0, 0x17, 0xf6, 0x71, 1, 2]).unwrap();
        assert!(is_manifest_file(&path));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn args_use_the_ugc_and_manifest_flags() {
        let args = depot_args(
            Path::new("dd.exe"),
            "123",
            Path::new("k.key"),
            Path::new("m.manifest"),
            Path::new("item"),
        );
        assert!(args.windows(2).any(|pair| pair == ["-ugc", "123"]));
        assert!(args.windows(2).any(|pair| pair == ["-manifestfile", "m.manifest"]));
        assert!(args.iter().any(|arg| arg == "-verify-all"));
    }
}
