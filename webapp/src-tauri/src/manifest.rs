//! Anonymous manifest downloads.
//!
//! Per item:
//!   1. workshop id -> hcontent_file (Steam's public details API)
//!   2. manifest id -> short-lived request code (gmrc.wudrm.com)
//!   3. code -> SteamPipe CDN -> ZIP with member "z" = raw depot manifest
//!   4. DepotDownloaderMod -ugc <id> -manifestfile <manifest> -depotkeys <key>
//!
//! No Steam account and no API key are involved. The code provider sees the
//! caller's IP and the manifest id only. Set the standard HTTPS_PROXY
//! environment variable to send that request through a proxy.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::blocking::Client;
use serde::Serialize;

use crate::settings;

pub const APP_ID: &str = "431960";
pub const DEPOT_ID: &str = "431960";
pub const MANIFEST_MAGIC: [u8; 4] = [0xd0, 0x17, 0xf6, 0x71];
const REQUEST_CODE_URL: &str = "https://gmrc.wudrm.com/manifest";
const CDN_NODES: [&str; 6] = [
    "cache6-par1.steamcontent.com",
    "cache1-lhr1.steamcontent.com",
    "cache1-ams1.steamcontent.com",
    "cache1-fra1.steamcontent.com",
    "cache10-sgp1.steamcontent.com",
    "cache2-hkg1.steamcontent.com",
];
/// Public depot key for 431960, from ManifestHub's key list. Keys are stable,
/// so embedding it avoids fetching their list on first run.
const DEPOT_KEY: &str = "db983bc58a8cb46d89d1d70cc962cc733956e6124c4025a0c932f82b3c37144a";
const KEY_FILE: &str = "431960.key";

const MAX_ATTEMPTS: usize = 4;
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
];
const USER_AGENT: &str = "WallForge/1.0 (anonymous manifest)";

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
    Fatal(String),
    Transient(String),
}

fn snippet(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(200)])
        .trim()
        .to_string()
}

/// Uses HTTPS_PROXY / HTTP_PROXY when set, so users can route the single
/// code request through their own proxy without configuring the app.
fn code_client() -> Result<Client, String> {
    Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|err| err.to_string())
}

/// The CDN is always contacted directly — only the code request is proxied.
fn cdn_client() -> Result<Client, String> {
    Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(60))
        .no_proxy()
        .build()
        .map_err(|err| err.to_string())
}

/// `workshop id -> hcontent_file`, via Steam's keyless details API.
pub fn resolve_gid(client: &Client, workshop_id: &str) -> Result<String, String> {
    let form = vec![
        ("itemcount".to_string(), "1".to_string()),
        ("publishedfileids[0]".to_string(), workshop_id.to_string()),
    ];
    let response = client
        .post(crate::steam::STEAM_API_URL)
        .header("Accept-Language", "en-US,en;q=0.9")
        .form(&form)
        .send()
        .map_err(|err| err.to_string())?;
    let payload: serde_json::Value = response.json().map_err(|err| err.to_string())?;
    let entry = payload
        .pointer("/response/publishedfiledetails/0")
        .ok_or_else(|| "Steam returned no details for this item.".to_string())?;
    if entry.get("result").and_then(serde_json::Value::as_i64) != Some(1) {
        return Err("That item is private, removed, or unavailable.".to_string());
    }
    let gid = entry
        .get("hcontent_file")
        .map(|value| match value {
            serde_json::Value::String(text) => text.clone(),
            serde_json::Value::Number(number) => number.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default();
    if gid.is_empty() || gid == "0" {
        return Err("That item has no depot manifest; it may be a collection.".to_string());
    }
    Ok(gid)
}

fn valid_code(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 20
        && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn request_code_once(client: &Client, gid: &str) -> Result<String, Attempt> {
    let url = format!("{REQUEST_CODE_URL}/{gid}");
    let response = client
        .get(&url)
        .send()
        .map_err(|err| Attempt::Transient(format!("network: {err}")))?;
    let status = response.status().as_u16();
    let body = response
        .bytes()
        .map_err(|err| Attempt::Transient(format!("network: {err}")))?;
    let text = String::from_utf8_lossy(&body).trim().to_string();

    if status == 400 {
        return Err(Attempt::Fatal(format!(
            "The code service rejected manifest {gid}: {}",
            snippet(&body)
        )));
    }
    if status == 404 {
        return Err(Attempt::Fatal(format!(
            "The code service has no code for manifest {gid}."
        )));
    }
    if status == 429 || (500..=599).contains(&status) {
        return Err(Attempt::Transient(format!("HTTP {status}")));
    }
    if status != 200 {
        return Err(Attempt::Fatal(format!(
            "Code service HTTP {status}: {}",
            snippet(&body)
        )));
    }
    if !valid_code(&text) {
        return Err(Attempt::Transient(format!(
            "unexpected code response {}",
            snippet(&body)
        )));
    }
    Ok(text)
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

/// Normalise a CDN body to raw VZ manifest bytes.
fn parse_manifest_payload(raw: &[u8]) -> Result<Vec<u8>, String> {
    if raw.get(..4) == Some(&MANIFEST_MAGIC) {
        return Ok(raw.to_vec());
    }
    if raw.get(..2) == Some(b"PK") {
        return unzip_manifest(raw);
    }
    Err(format!("unrecognised manifest payload ({} bytes)", raw.len()))
}

/// Fetch a manifest: code from the provider, bytes from a SteamPipe CDN node.
/// Network/429/5xx failures retry 4 times at 2/4/8s across all nodes.
pub fn fetch_manifest(gid: &str, log: impl Fn(&str)) -> Result<Vec<u8>, FetchError> {
    let code_client = code_client().map_err(|err| FetchError::new("FATAL", err, false))?;
    let cdn = cdn_client().map_err(|err| FetchError::new("FATAL", err, false))?;

    let mut last = String::new();
    for attempt in 0..MAX_ATTEMPTS {
        let result = (|| -> Result<Vec<u8>, Attempt> {
            let code = request_code_once(&code_client, gid)?;
            let path = format!("/depot/{DEPOT_ID}/manifest/{gid}/5/{code}");
            let mut problems = Vec::new();
            for node in CDN_NODES {
                let url = format!("https://{node}{path}");
                let response = match cdn.get(&url).send() {
                    Ok(response) => response,
                    Err(err) => {
                        problems.push(format!("{node}: network: {err}"));
                        continue;
                    }
                };
                let status = response.status().as_u16();
                let body = match response.bytes() {
                    Ok(body) => body,
                    Err(err) => {
                        problems.push(format!("{node}: read: {err}"));
                        continue;
                    }
                };
                if status == 200 && !body.is_empty() {
                    return parse_manifest_payload(&body).map_err(Attempt::Fatal);
                }
                problems.push(format!("{node}: HTTP {status}"));
            }
            Err(Attempt::Transient(problems.join("; ")))
        })();

        match result {
            Ok(bytes) => return Ok(bytes),
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
                    "Manifest fetch hiccup - retry {}/{} in {}s",
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
        format!("Manifest fetch failed after {MAX_ATTEMPTS} attempts: {last}"),
        true,
    ))
}

/* ---------------- local files ---------------- */

fn key_file_path() -> PathBuf {
    settings::data_dir().join(KEY_FILE)
}

fn key_file_has_depot(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    text.lines()
        .any(|line| line.trim_start().starts_with(&format!("{DEPOT_ID};")))
}

/// Path to a key file containing the Wallpaper Engine depot key.
pub fn ensure_key_file() -> Result<PathBuf, String> {
    let path = key_file_path();
    if key_file_has_depot(&path) {
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::write(&path, format!("{DEPOT_ID};{DEPOT_KEY}\n")).map_err(|err| err.to_string())?;
    Ok(path)
}

fn cache_dir(item_dir: &Path) -> PathBuf {
    item_dir.join(".DepotDownloader")
}

pub fn manifest_name(gid: &str) -> String {
    format!("{DEPOT_ID}_{gid}.manifest")
}

pub fn is_manifest_file(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.get(..4) == Some(&MANIFEST_MAGIC)
}

/// A manifest cached beside the item never needs the provider again.
pub fn cached_manifest(item_dir: &Path, gid: &str) -> Option<PathBuf> {
    let path = cache_dir(item_dir).join(manifest_name(gid));
    if is_manifest_file(&path) {
        Some(path)
    } else {
        None
    }
}

/// Write through %TEMP%, then keep the only copy in the item's .DepotDownloader.
pub fn stage_manifest(item_dir: &Path, gid: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let temp = std::env::temp_dir().join(format!("wallforge_{gid}.manifest"));
    let final_path = cache_dir(item_dir).join(manifest_name(gid));
    fs::write(&temp, bytes).map_err(|err| format!("Could not stage the manifest: {err}"))?;
    let result = (|| -> Result<(), String> {
        fs::create_dir_all(cache_dir(item_dir)).map_err(|err| err.to_string())?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_decimal_strings() {
        assert!(valid_code("12345"));
        assert!(valid_code(&"9".repeat(20)));
        assert!(!valid_code(""));
        assert!(!valid_code(&"9".repeat(21)));
        assert!(!valid_code("12ab"));
    }

    #[test]
    fn manifest_payloads_are_normalised() {
        assert_eq!(parse_manifest_payload(&[0xd0, 0x17, 0xf6, 0x71, 1]).unwrap().len(), 5);
        assert!(parse_manifest_payload(b"not a manifest").is_err());
    }

    #[test]
    fn depot_arguments_use_ugc_and_manifest() {
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
