//! WallForge: a browser for the Wallpaper Engine Workshop.
//!
//! The React interface calls these commands. Everything Steam related lives
//! here: browsing the Workshop, item details, downloads, sign-in and the
//! local library.

mod accounts;
mod depot_bundle;
mod downloads;
mod library;
mod manifest;
mod settings;
mod steam;
mod windows_crypto;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{Manager, State};

use accounts::import_shared_accounts;
use downloads::Depot;
use settings::{is_built_in, Settings};

pub struct AppState {
    depot: Arc<Depot>,
    client: reqwest::Client,
    settings: Arc<Mutex<Settings>>,
}

impl AppState {
    fn settings(&self) -> Settings {
        self.settings.lock().map(|value| value.clone()).unwrap_or_default()
    }

    fn update_settings(&self, change: impl FnOnce(&mut Settings)) {
        if let Ok(mut settings) = self.settings.lock() {
            change(&mut settings);
            settings.save();
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountEntry {
    pub username: String,
    pub signed_in: bool,
    pub has_password: bool,
    pub built_in: bool,
}

fn account_entries(settings: &Settings) -> Vec<AccountEntry> {
    settings
        .accounts
        .iter()
        .map(|account| AccountEntry {
            username: account.username.clone(),
            signed_in: account.signed_in,
            has_password: settings
                .credentials
                .contains_key(&account.username.to_lowercase()),
            built_in: is_built_in(&account.username),
        })
        .collect()
}

fn install_dir(settings: &Settings) -> Option<PathBuf> {
    library::active_install_dir(settings)
}

#[tauri::command]
fn health() -> Value {
    json!({"ok": true, "version": env!("CARGO_PKG_VERSION")})
}

#[tauri::command]
fn get_info(state: State<'_, AppState>) -> Value {
    let settings = state.settings();
    let dir = install_dir(&settings);
    let source = if settings.install_dir.is_some() {
        "manual"
    } else if dir.is_some() {
        "auto"
    } else {
        "none"
    };
    let install_path = dir
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    let download_path = dir
        .as_ref()
        .map(|path| library::wallpaper_dir(path).to_string_lossy().to_string());
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "install_dir": install_path,
        "auto_install_dir": library::find_install_dir().map(|path| path.to_string_lossy().to_string()),
        "auto_candidates": Vec::<String>::new(),
        "install_source": source,
        "depot_exists": install_path.is_some(),
        "download_dir": download_path,
    })
}

#[tauri::command]
fn get_accounts(state: State<'_, AppState>) -> Value {
    let settings = state.settings();
    json!({
        "accounts": account_entries(&settings),
        "active": settings.active_account(),
    })
}

#[tauri::command]
fn select_account(username: String, state: State<'_, AppState>) -> Value {
    let mut ok = false;
    state.update_settings(|settings| {
        ok = settings.set_active_account(&username);
    });
    let settings = state.settings();
    json!({
        "success": ok,
        "message": if ok { String::new() } else { "That account is not saved.".to_string() },
        "accounts": account_entries(&settings),
        "active": settings.active_account(),
    })
}

#[tauri::command]
fn save_account_password(username: String, password: String, state: State<'_, AppState>) -> Value {
    let settings = state.settings();
    if is_built_in(&username) {
        return json!({
            "success": false,
            "message": "That account is built into WallForge, so its password cannot be changed.",
            "accounts": account_entries(&settings),
            "active": settings.active_account(),
        });
    }
    if username.trim().is_empty() || password.is_empty() {
        return json!({
            "success": false,
            "message": "Could not store that password on this PC.",
            "accounts": account_entries(&settings),
            "active": settings.active_account(),
        });
    }
    let blob = match windows_crypto::protect_secret(&password) {
        Ok(blob) => blob,
        Err(err) => {
            return json!({
                "success": false,
                "message": format!("Could not store that password: {err}"),
                "accounts": account_entries(&settings),
                "active": settings.active_account(),
            })
        }
    };
    state.update_settings(|settings| {
        settings
            .credentials
            .insert(username.trim().to_lowercase(), blob);
    });
    let settings = state.settings();
    json!({
        "success": true,
        "message": "",
        "accounts": account_entries(&settings),
        "active": settings.active_account(),
    })
}

#[tauri::command]
fn delete_account(username: String, state: State<'_, AppState>) -> Value {
    if is_built_in(&username) {
        let settings = state.settings();
        return json!({
            "success": false,
            "message": "That account is built into WallForge and cannot be removed.",
            "accounts": account_entries(&settings),
            "active": settings.active_account(),
        });
    }
    let mut removed = false;
    state.update_settings(|settings| {
        removed = settings.remove_account(&username);
    });
    let settings = state.settings();
    json!({
        "success": removed,
        "message": if removed { String::new() } else { "That account was not saved.".to_string() },
        "accounts": account_entries(&settings),
        "active": settings.active_account(),
    })
}

#[tauri::command]
async fn get_filters(state: State<'_, AppState>) -> Result<Value, String> {
    let groups = steam::fetch_filter_groups(&state.client).await.unwrap_or_default();
    Ok(json!({ "groups": groups }))
}

#[tauri::command]
async fn search_workshop(
    searchtext: String,
    page: i64,
    sortmethod: String,
    timeperiod: String,
    required_tags: Option<Vec<String>>,
    excluded_tags: Option<Vec<String>>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let tags = required_tags.unwrap_or_default();
    let excluded = excluded_tags.unwrap_or_default();
    let result = steam::fetch_top_wallpapers(
        &state.client,
        page,
        &searchtext,
        &sortmethod,
        &timeperiod,
        30,
        &tags,
        &excluded,
    )
    .await?;
    Ok(result)
}

#[tauri::command]
async fn get_item(workshop_id: String, state: State<'_, AppState>) -> Result<Value, String> {
    if !steam::valid_workshop_id(&workshop_id) {
        return Err("Invalid Workshop ID".to_string());
    }
    let mut items = steam::fetch_items_metadata(&state.client, &[workshop_id.clone()]).await?;
    items
        .remove(&workshop_id)
        .ok_or_else(|| "That item could not be loaded.".to_string())
}

/// The wallpapers published by one author, from their Workshop files page.
/// The Workshop search cannot filter by creator, so this is the only
/// in-app way to browse an author's work.
#[tauri::command]
async fn author_wallpapers(
    author_url: String,
    page: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    if !steam::valid_author_url(&author_url) {
        return Err("That link does not look like a Steam author page.".to_string());
    }
    let page = page.unwrap_or(1).max(1);
    let base = author_url.trim_end_matches('/');
    let url = format!(
        "{base}/myworkshopfiles/?appid=431960&numperpage={}&p={page}",
        steam::AUTHOR_PAGE_SIZE
    );
    let response = steam::with_language(state.client.get(&url))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let html = response.text().await.map_err(|err| err.to_string())?;
    let items = steam::items_from_legacy_html(&html);
    let (total_count, total_pages) = steam::listing_totals(&html, steam::AUTHOR_PAGE_SIZE);
    Ok(json!({
        "items": items,
        "page": page,
        "total_pages": total_pages,
        "total_count": total_count,
    }))
}

#[tauri::command]
async fn get_item_details(workshop_id: String, state: State<'_, AppState>) -> Result<Value, String> {
    if !steam::valid_workshop_id(&workshop_id) {
        return Err("Invalid Workshop ID".to_string());
    }
    match steam::fetch_item_details(&state.client, &workshop_id).await {
        Ok(value) => Ok(value),
        Err(err) => {
            // An age gate is an answer, not a failure — keep it rather than
            // papering over it with the Web API fallback.
            if err.contains("age restricted") {
                return Err(err);
            }
            // Steam sometimes serves a page that carries no item markup
            // (rate limiting, interstitials). Fall back to the official Web
            // API so the basics still show instead of a dead end.
            let found = steam::fetch_items_metadata(&state.client, std::slice::from_ref(&workshop_id)).await;
            if let Ok(found) = found {
                if let Some(item) = found.get(&workshop_id) {
                    return Ok(json!({
                        "id": workshop_id,
                        "title": item.get("title").cloned().unwrap_or(Value::String(workshop_id.clone())),
                        "author": "",
                        "author_url": "",
                        "description": "",
                        "img": item.get("img").cloned().unwrap_or(Value::Null),
                        "images": [],
                        "video_id": null,
                        "link": item.get("link").cloned().unwrap_or(Value::Null),
                        "rating": "",
                        "rating_count": "",
                        "file_size": "",
                        "posted": "",
                        "updated": "",
                        "tags": [],
                        "partial": true,
                    }));
                }
            }
            Err(err)
        }
    }
}

#[tauri::command]
async fn list_downloads(
    sort: Option<String>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let settings = state.settings();
    let dir = install_dir(&settings);
    let mut items = library::list_local_downloads(dir.as_deref(), sort.as_deref().unwrap_or("newest"));

    // Wallpapers downloaded by an older version have no details file, so they
    // show up without a picture. Ask Steam for the missing ones and keep
    // the answer on disk, so this only costs a request once.
    let missing: Vec<String> = items
        .iter()
        .filter(|item| {
            item.get("img")
                .and_then(Value::as_str)
                .map(|img| img.is_empty())
                .unwrap_or(true)
        })
        .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
        .collect();

    if !missing.is_empty() {
        if let Ok(found) = steam::fetch_items_metadata(&state.client, &missing).await {
            for item in items.iter_mut() {
                let id = item.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                let Some(meta) = found.get(&id) else {
                    continue;
                };
                let folder = item.get("path").and_then(Value::as_str).unwrap_or_default();
                if !folder.is_empty() {
                    library::write_meta(Path::new(folder), meta);
                }
                if let Some(map) = item.as_object_mut() {
                    for key in ["title", "author", "img", "link", "rating"] {
                        if let Some(value) = meta.get(key) {
                            map.insert(key.to_string(), value.clone());
                        }
                    }
                }
            }
        }
    }

    Ok(json!({ "items": items }))
}

#[tauri::command]
fn download(
    workshop_id: String,
    title: Option<String>,
    meta: Option<Value>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    if !steam::valid_workshop_id(&workshop_id) {
        return Err("Invalid Workshop ID".to_string());
    }
    let settings = state.settings();
    let account = settings
        .active_account()
        .ok_or_else(|| "Sign in to Steam first - click the Steam button in the header".to_string())?;
    let dir = install_dir(&settings)
        .ok_or_else(|| "Wallpaper Engine folder not found or Workshop ID is invalid".to_string())?;
    let target = library::wallpaper_dir(&dir).join(&workshop_id);
    let password = settings
        .credentials
        .get(&account.to_lowercase())
        .map(|blob| windows_crypto::unprotect_secret(blob))
        .unwrap_or_default();

    // Anonymous mode tries a public manifest first; any failure falls back
    // to this same account.
    state.depot.enqueue(
        &workshop_id,
        &title.unwrap_or_default(),
        target,
        &account,
        &password,
        meta,
        settings.manifest_enabled,
        None,
    );
    Ok(json!({
        "success": true,
        "started": true,
        "job_id": workshop_id,
    }))
}

#[tauri::command]
fn download_queue(state: State<'_, AppState>) -> Value {
    json!({ "jobs": state.depot.jobs_snapshot() })
}

#[tauri::command]
fn download_pause(job_id: String, state: State<'_, AppState>) -> Value {
    json!({ "success": state.depot.pause(&job_id) })
}

#[tauri::command]
fn download_resume(job_id: String, state: State<'_, AppState>) -> Value {
    json!({ "success": state.depot.resume(&job_id) })
}

#[tauri::command]
fn download_cancel(job_id: String, state: State<'_, AppState>) -> Value {
    json!({ "success": state.depot.cancel(&job_id) })
}

#[tauri::command]
fn download_forget(job_id: String, state: State<'_, AppState>) -> Value {
    json!({ "success": state.depot.forget(&job_id) })
}

#[tauri::command]
fn download_clear(state: State<'_, AppState>) -> Value {
    json!({ "success": true, "cleared": state.depot.clear_list() })
}

#[tauri::command]
fn delete_download(workshop_id: String, state: State<'_, AppState>) -> Value {
    let settings = state.settings();
    let dir = install_dir(&settings);
    json!({ "success": library::delete_download(dir.as_deref(), &workshop_id) })
}


#[tauri::command]
fn open_folder(workshop_id: String, state: State<'_, AppState>) -> Value {
    let settings = state.settings();
    let dir = install_dir(&settings);
    match library::open_folder(dir.as_deref(), &workshop_id) {
        Ok(()) => json!({"success": true, "message": ""}),
        Err(message) => json!({"success": false, "message": message}),
    }
}

/// Open a Steam Workshop link in the default browser.
#[tauri::command]
fn open_url(url: String) -> Value {
    match library::shell_open_url(&url) {
        Ok(()) => json!({"success": true, "message": ""}),
        Err(message) => json!({"success": false, "message": message}),
    }
}

/* ---------------- experimental anonymous manifests ---------------- */

#[tauri::command]
fn experimental_status(state: State<'_, AppState>) -> Value {
    json!({ "enabled": state.settings().manifest_enabled })
}

#[tauri::command]
fn experimental_set_enabled(enabled: bool, state: State<'_, AppState>) -> Value {
    state.update_settings(|settings| settings.manifest_enabled = enabled);
    json!({"success": true, "enabled": enabled})
}

/// Open the native folder picker; returns the chosen path without saving it.
#[tauri::command]
async fn browse_install_dir(app: tauri::AppHandle) -> Value {
    use tauri_plugin_dialog::DialogExt;

    let (sender, receiver) = std::sync::mpsc::channel::<Option<String>>();
    app.dialog().file().pick_folder(move |path| {
        let chosen = path.map(|path| path.to_string());
        let _ = sender.send(chosen);
    });

    let picked = receiver.recv().ok().flatten();
    match picked {
        Some(path) => json!({"success": true, "cancelled": false, "path": path, "message": ""}),
        None => json!({"success": true, "cancelled": true, "path": null, "message": ""}),
    }
}

#[tauri::command]
fn set_install_dir(path: String, state: State<'_, AppState>) -> Value {
    let candidate = PathBuf::from(path.trim());
    if !library::looks_like_we_install(&candidate) {
        return json!({
            "success": false,
            "message": "That folder does not look like a Wallpaper Engine install. Pick the folder that contains 'projects' or 'wallpaper32.exe'.",
        });
    }
    state.update_settings(|settings| {
        settings.install_dir = Some(candidate.to_string_lossy().to_string());
    });
    json!({
        "success": true,
        "message": format!("Using {}", candidate.to_string_lossy()),
    })
}

#[tauri::command]
fn reset_install_dir(state: State<'_, AppState>) -> Value {
    state.update_settings(|settings| {
        settings.install_dir = None;
    });
    json!({"success": true, "message": "Using the folder WallForge detected."})
}

#[tauri::command]
fn start_login(
    method: String,
    username: String,
    password: String,
    state: State<'_, AppState>,
) -> Value {
    match downloads::start_login(&state.depot, &method, &username, &password) {
        Ok(()) => {
            if !username.trim().is_empty() {
                state.update_settings(|settings| {
                    settings.save_account(&username);
                });
            }
            let login = state.depot.login_state();
            json!({
                "status": login.status,
                "message": login.message,
                "username": login.username,
                "success": true,
            })
        }
        Err(message) => {
            let mut login = state.depot.login_state();
            login.status = "error".to_string();
            login.message = message.clone();
            json!({
                "status": "error",
                "message": message,
                "username": login.username,
                "success": false,
            })
        }
    }
}

#[tauri::command]
fn login_status(state: State<'_, AppState>) -> Value {
    let login = state.depot.login_state();
    let settings = state.settings();
    json!({
        "status": login.status,
        "message": login.message,
        "username": login.username,
        "has_qr": !login.qr.is_empty(),
        "qr_version": login.qr_version,
        "accounts": account_entries(&settings),
        "active": settings.active_account(),
    })
}

#[tauri::command]
fn submit_login_code(code: String, state: State<'_, AppState>) -> Value {
    match downloads::submit_login_code(&state.depot, &code) {
        Ok(()) => json!({"success": true, "message": "Code sent to Steam."}),
        Err(message) => json!({"success": false, "message": message}),
    }
}

#[tauri::command]
fn cancel_login(state: State<'_, AppState>) -> Value {
    downloads::cancel_login(&state.depot);
    json!({"success": true})
}

/// The QR code as a data url the interface can show directly.
#[tauri::command]
fn qr_svg(state: State<'_, AppState>) -> Value {
    let login = state.depot.login_state();
    let svg = downloads::qr_svg(&login.qr);
    if svg.is_empty() {
        return json!({"success": false, "message": "No QR code yet"});
    }
    json!({
        "success": true,
        "data_url": format!(
                "data:image/svg+xml;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(svg)
            ),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let mut settings = Settings::load();
            let armed = import_shared_accounts(&mut settings);
            if armed > 0 {
                settings.save();
            }
            // The download tool travels inside the binary and is written out
            // on first run, so the app ships as a single file.
            if let Err(message) = depot_bundle::ensure_extracted() {
                eprintln!("{message}");
            }
            let depot = Arc::new(Depot::new(depot_bundle::depot_exe_path()));
            depot.start_worker();
            app.manage(AppState {
                depot,
                client: steam::http_client().map_err(std::io::Error::other)?,
                settings: Arc::new(Mutex::new(settings)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health,
            get_info,
            get_accounts,
            select_account,
            save_account_password,
            delete_account,
            get_filters,
            search_workshop,
            get_item,
            get_item_details,
            author_wallpapers,
            list_downloads,
            download,
            download_queue,
            download_pause,
            download_resume,
            download_cancel,
            download_forget,
            download_clear,
            delete_download,
            open_folder,
            open_url,
            experimental_status,
            experimental_set_enabled,
            browse_install_dir,
            set_install_dir,
            reset_install_dir,
            start_login,
            login_status,
            submit_login_code,
            cancel_login,
            qr_svg,
        ])
        .run(tauri::generate_context!())
        .expect("error while running WallForge");
}
