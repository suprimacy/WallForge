//! Wallpaper Engine detection and the local wallpaper library.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{json, Value};
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::HKEY;

use crate::settings::Settings;
use crate::steam::APP_ID;

/// The wallpaper folder inside the install directory.
///
/// The parts are joined one by one, so the path uses the separator Windows
/// expects. A single "projects/myprojects" string leaves a mixed path
/// behind, which the shell refuses to open.
pub fn wallpaper_dir(install_dir: &Path) -> PathBuf {
    install_dir.join("projects").join("myprojects")
}

/// Read a string value from the Windows registry.
fn registry_string(root: HKEY, path: &str, name: &str) -> Option<String> {
    let key = winreg::RegKey::predef(root).open_subkey(path).ok()?;
    let value: String = key.get_value(name).ok()?;
    Some(value)
}

/// Wallpaper Engine's install folder, from the registry or Steam.
pub fn find_install_dir() -> Option<PathBuf> {
    if let Some(path) = registry_string(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\WOW6432Node\Valve\Steam",
        "InstallPath",
    )
    .or_else(|| {
        registry_string(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Valve\Steam",
            "InstallPath",
        )
    })
    .or_else(|| {
        registry_string(HKEY_CURRENT_USER, r"SOFTWARE\Valve\Steam", "SteamPath")
    }) {
        let steam = PathBuf::from(path);
        for candidate in steam_library_roots(&steam) {
            for steamapps_name in ["steamapps", "SteamApps"] {
                let steamapps = candidate.join(steamapps_name);
                if !steamapps.is_dir() {
                    continue;
                }
                let manifest = steamapps.join(format!("appmanifest_{APP_ID}.acf"));
                if manifest.is_file() {
                    if let Ok(content) = fs::read_to_string(&manifest) {
                        if let Some(installdir) = Regex::new(r#""installdir"\s+"(.+?)""#)
                            .ok()
                            .and_then(|regex| regex.captures(&content))
                        {
                            let folder = steamapps.join("common").join(&installdir[1]);
                            if folder.is_dir() {
                                return Some(folder);
                            }
                        }
                    }
                }
                // A leftover manifest is common; the folder itself is the proof.
                let fallback = steamapps.join("common").join("Wallpaper Engine");
                if looks_like_we_install(&fallback) {
                    return Some(fallback);
                }
            }
        }
    }
    for candidate in [
        PathBuf::from(r"C:\Program Files (x86)\Steam"),
        PathBuf::from(r"C:\Program Files\Steam"),
    ] {
        let we = candidate.join("common").join("Wallpaper Engine");
        if looks_like_we_install(&we) {
            return Some(we);
        }
    }
    None
}

/// The Steam library folders, from libraryfolders.vdf.
pub fn steam_library_roots(steam: &Path) -> Vec<PathBuf> {
    let mut roots = vec![steam.to_path_buf()];
    let mut seen: Vec<String> = vec![key_of(steam)];
    let pattern = Regex::new(r#""path"\s+"([^"]+)""#).ok();
    let Some(pattern) = pattern else {
        return roots;
    };
    for vdf in [
        steam.join("steamapps").join("libraryfolders.vdf"),
        steam.join("config").join("libraryfolders.vdf"),
    ] {
        let Ok(text) = fs::read_to_string(&vdf) else {
            continue;
        };
        for capture in pattern.captures_iter(&text) {
            // The VDF escapes backslashes, e.g. "D:\\SteamLibrary".
            let candidate = PathBuf::from(capture[1].replace("\\\\", "\\"));
            let key = key_of(&candidate);
            if seen.contains(&key) || !candidate.is_dir() {
                continue;
            }
            seen.push(key);
            roots.push(candidate);
        }
    }
    roots
}

/// A comparable form of a path, so the same folder is not added twice.
fn key_of(path: &Path) -> String {
    path.to_string_lossy()
        .to_lowercase()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_string()
}

/// A folder is usable when it holds the projects tree or the Wallpaper Engine exe.
pub fn looks_like_we_install(path: &Path) -> bool {
    if wallpaper_dir(path).is_dir() || path.join("projects").is_dir() {
        return true;
    }
    ["wallpaper32.exe", "wallpaper64.exe"]
        .iter()
        .any(|exe| path.join(exe).is_file())
}

/// The folder downloaded wallpapers live in.
pub fn wallpaper_base_dir(install_dir: &Path) -> Option<PathBuf> {
    let target = wallpaper_dir(install_dir);
    fs::create_dir_all(&target).ok()?;
    Some(target)
}

/// The wallpaper folders in the library, newest or oldest first.
///
/// A wallpaper that has no details file yet is reported with an empty
/// picture, so the caller can fill it in from Steam.
pub fn list_local_downloads(install_dir: Option<&Path>, sort: &str) -> Vec<Value> {
    let base = match install_dir {
        Some(dir) => wallpaper_base_dir(dir),
        None => None,
    };
    let base = match base {
        Some(path) => path,
        None => return Vec::new(),
    };
    let entries = match fs::read_dir(&base) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut items: Vec<Value> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };
        if !crate::steam::valid_workshop_id(&id) {
            continue;
        }
        let meta = read_meta(&path);
        let added = folder_created(&path);
        items.push(json!({
            "id": id,
            "title": meta.get("title").and_then(Value::as_str).unwrap_or(&id),
            "author": meta.get("author").and_then(Value::as_str).unwrap_or(""),
            "img": meta.get("img").and_then(Value::as_str).unwrap_or(""),
            "link": format!("{}/?id={id}", crate::steam::WORKSHOP_ITEM_URL),
            "rating": "",
            "path": path.to_string_lossy(),
            "pathShort": format!("\\projects\\myprojects\\{id}"),
            "added": added,
        }));
    }

    if sort == "oldest" {
        items.sort_by(|a, b| added_of(a).partial_cmp(&added_of(b)).unwrap_or(std::cmp::Ordering::Equal));
    } else {
        items.sort_by(|a, b| added_of(b).partial_cmp(&added_of(a)).unwrap_or(std::cmp::Ordering::Equal));
    }
    items
}

fn added_of(item: &Value) -> f64 {
    item.get("added").and_then(Value::as_f64).unwrap_or(0.0)
}

fn read_meta(folder: &Path) -> Value {
    let path = folder.join("meta.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return Value::Null;
    };
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

/// Write the details of a downloaded wallpaper next to its files.
///
/// The library reads this file to show the title, the creator and the
/// preview. Without it, a wallpaper that was downloaded by a different
/// version of the app shows up with no picture.
pub fn write_meta(folder: &Path, meta: &Value) -> bool {
    let path = folder.join("meta.json");
    let Ok(text) = serde_json::to_string_pretty(meta) else {
        return false;
    };
    fs::write(&path, text).is_ok()
}

/// When the wallpaper folder was created, i.e. when it was downloaded.
fn folder_created(path: &Path) -> f64 {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return 0.0,
    };
    let created = metadata
        .created()
        .ok()
        .or_else(|| metadata.modified().ok());
    created
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

/// Delete a downloaded wallpaper folder.
pub fn delete_download(install_dir: Option<&Path>, workshop_id: &str) -> bool {
    let base = match install_dir {
        Some(dir) => wallpaper_base_dir(dir),
        None => None,
    };
    let Some(base) = base else { return false };
    if !crate::steam::valid_workshop_id(workshop_id) {
        return false;
    }
    let target = base.join(workshop_id);
    if !target.is_dir() {
        return false;
    }
    fs::remove_dir_all(&target).is_ok()
}

/// Open a wallpaper folder in Explorer.
pub fn open_folder(install_dir: Option<&Path>, workshop_id: &str) -> Result<(), String> {
    let base = match install_dir {
        Some(dir) => wallpaper_base_dir(dir),
        None => None,
    };
    let Some(base) = base else {
        return Err("Wallpaper Engine folder not found.".to_string());
    };
    let target = base.join(workshop_id);
    if !target.is_dir() {
        return Err("Folder not found".to_string());
    }
    shell_open(&target)
}

/// Hand a URL to the Windows shell, which opens it in the default browser.
/// Only https links off Steam's own hosts are allowed, so the app cannot
/// be used to launch a `file:` or `javascript:` URL.
pub fn shell_open_url(url: &str) -> Result<(), String> {
    shell_open_allowlisted(url, &["steamcommunity.com", "steampowered.com"])
}

fn shell_open_allowlisted(url: &str, hosts: &[&str]) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW};

    /// SW_SHOWNORMAL, the plain window size and position.
    const SHOW_NORMAL: i32 = 1;

    let trimmed = url.trim();
    let lowered = trimmed.to_lowercase();
    let allowed = hosts.iter().any(|host| lowered.contains(host));
    if !lowered.starts_with("https://") || !allowed {
        return Err("That link is not on an allowed website.".to_string());
    }

    let wide = |value: &str| -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    };
    let operation = wide("open");
    let target = wide(trimmed);

    let mut request = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        lpVerb: PCWSTR::from_raw(operation.as_ptr()),
        lpFile: PCWSTR::from_raw(target.as_ptr()),
        nShow: SHOW_NORMAL,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut request) }
        .map_err(|err| format!("Windows would not open that link: {err}"))
}

/// Hand a folder to the Windows shell, which opens it in Explorer.
///
/// `explorer.exe` is not used on purpose: it refuses a path that mixes
/// separators, and the wallpaper folder is built from a relative path that
/// contains a forward slash. The shell accepts it and reports whether it
/// worked, so a failure can be shown instead of being swallowed.
fn shell_open(path: &Path) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW};

    /// SW_SHOWNORMAL, the plain window size and position.
    const SHOW_NORMAL: i32 = 1;

    let wide = |value: &str| -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    };
    let operation = wide("open");
    let file = wide(&path.to_string_lossy().replace('/', "\\"));

    let mut request = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        lpVerb: PCWSTR::from_raw(operation.as_ptr()),
        lpFile: PCWSTR::from_raw(file.as_ptr()),
        nShow: SHOW_NORMAL,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut request) }
        .map_err(|err| format!("Windows would not open the folder: {err}"))
}

/// The install folder, preferring the one the user picked.
pub fn active_install_dir(settings: &Settings) -> Option<PathBuf> {
    if let Some(dir) = settings.install_dir.as_deref() {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            return Some(path);
        }
    }
    find_install_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_detection_needs_projects_or_exe() {
        assert!(!looks_like_we_install(Path::new("C:\\nope\\not-here")));
    }

    #[test]
    fn the_registry_has_the_steam_path() {
        // The Wallpaper Engine folder is found through the registry, so this
        // must keep working on a normal install.
        let steam = registry_string(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        );
        println!("steam path: {steam:?}");
        assert!(steam.is_some(), "Steam InstallPath not found in the registry");
    }

    #[test]
    fn invalid_ids_are_rejected() {
        assert!(!crate::steam::valid_workshop_id("../etc"));
        assert!(crate::steam::valid_workshop_id("123456"));
    }
}
