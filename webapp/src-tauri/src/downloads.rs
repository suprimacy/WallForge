//! Downloads and Steam sign-in, both driven by DepotDownloaderMod.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// One at a time: Steam throttles the account's sessions, so running jobs
/// in parallel gets the whole batch rate-limited.
const MAX_CONCURRENT_DOWNLOADS: usize = 1;
const APP_ID: &str = "431960";

/// CREATE_NO_WINDOW, so the download tool never flashes a console.
///
/// The tool is a console program, and this app is a windowed one. Without
/// the flag, Windows gives the child a console of its own and a black
/// command window appears in the background for every download.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Start the download tool with its console hidden.
fn depot_command(exe: &str) -> Command {
    let mut command = Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Where a download is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Downloading,
    Paused,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct Job {
    pub id: String,
    /// Shown in the download queue.
    #[allow(dead_code)]
    pub title: String,
    /// The wallpaper's picture, so the queue shows what is downloading.
    #[allow(dead_code)]
    pub img: Option<String>,
    pub state: JobState,
    pub progress: f64,
    pub message: String,
    pub path: Option<String>,
    /// Set on a rejected ManifestHub key so the interface can prompt for a new one.
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LoginState {
    pub status: String,
    pub message: String,
    pub username: Option<String>,
    pub qr: Vec<String>,
    pub qr_version: u32,
}

impl LoginState {
    pub fn new() -> LoginState {
        LoginState {
            status: "idle".to_string(),
            message: String::new(),
            username: None,
            qr: Vec::new(),
            qr_version: 0,
        }
    }
}

/// What the worker needs to run one download.
#[derive(Debug, Clone)]
struct JobRequest {
    id: String,
    title: String,
    target_dir: PathBuf,
    account: String,
    password: String,
    /// The details of the item, kept so the library can show it later.
    meta: Option<serde_json::Value>,
    /// `Some` means try the experimental ManifestHub path first; the string is
    /// the key (empty when missing/expired). `None` uses the Steam account.
    manifest_token: Option<String>,
    /// Why a manifest download fell back to the account, shown in the queue.
    note: Option<String>,
}

/// The live state of the running download.
struct Running {
    handle: Option<isize>,
    paused: bool,
    cancelled: bool,
}

pub struct Depot {
    pub exe: PathBuf,
    pub jobs: Arc<Mutex<Vec<Job>>>,
    pub login: Arc<Mutex<LoginState>>,
    pub login_stdin: Arc<Mutex<Option<ChildStdin>>>,
    queue: Arc<Mutex<Vec<JobRequest>>>,
    /// The live download for each in-flight job id.
    running: Arc<Mutex<HashMap<String, Running>>>,
}

impl Depot {
    pub fn new(exe: PathBuf) -> Depot {
        Depot {
            exe,
            jobs: Arc::new(Mutex::new(Vec::new())),
            login: Arc::new(Mutex::new(LoginState::new())),
            login_stdin: Arc::new(Mutex::new(None)),
            queue: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// The current sign-in state.
    pub fn login_state(&self) -> LoginState {
        self.login.lock().map(|state| state.clone()).unwrap_or_default()
    }

    /// Add a download to the queue.
    #[allow(clippy::too_many_arguments)]
    pub fn enqueue(
        &self,
        id: &str,
        title: &str,
        target_dir: PathBuf,
        account: &str,
        password: &str,
        meta: Option<serde_json::Value>,
        manifest_token: Option<String>,
        note: Option<String>,
    ) {
        {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(existing) = jobs.iter_mut().find(|job| job.id == id) {
                if matches!(
                    existing.state,
                    JobState::Queued | JobState::Downloading | JobState::Paused
                ) {
                    return;
                }
            }
            jobs.retain(|job| job.id != id);
            let picture = meta
                .as_ref()
                .and_then(|meta| meta.get("img"))
                .and_then(|img| img.as_str())
                .map(str::to_string);
            jobs.push(Job {
                id: id.to_string(),
                title: title.to_string(),
                img: picture,
                state: JobState::Queued,
                progress: 0.0,
                message: "Queued".to_string(),
                path: None,
                error_code: None,
            });
        }
        {
            let mut queue = self.queue.lock().unwrap();
            queue.retain(|job| job.id != id);
            queue.push(JobRequest {
                id: id.to_string(),
                title: title.to_string(),
                target_dir,
                account: account.to_string(),
                password: password.to_string(),
                meta,
                manifest_token,
                note,
            });
        }
    }

    /// The queue as the interface shows it.
    pub fn jobs_snapshot(&self) -> Vec<Job> {
        self.jobs.lock().map(|jobs| jobs.clone()).unwrap_or_default()
    }

    fn update(&self, id: &str, change: impl FnOnce(&mut Job)) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(job) = jobs.iter_mut().find(|job| job.id == id) {
                change(job);
            }
        }
    }

    /// Pause a download that is running.
    pub fn pause(&self, id: &str) -> bool {
        let mut running = match self.running.lock() {
            Ok(running) => running,
            Err(_) => return false,
        };
        let Some(live) = running.get_mut(id) else {
            return false;
        };
        if live.paused {
            return false;
        }
        live.paused = true;
        if let Some(handle) = live.handle {
            unsafe { suspend_process(handle) };
        }
        self.update(id, |job| {
            job.state = JobState::Paused;
            job.message = "Paused".to_string();
        });
        true
    }

    /// Continue a paused download.
    pub fn resume(&self, id: &str) -> bool {
        let mut running = match self.running.lock() {
            Ok(running) => running,
            Err(_) => return false,
        };
        let Some(live) = running.get_mut(id) else {
            return false;
        };
        if !live.paused {
            return false;
        }
        live.paused = false;
        if let Some(handle) = live.handle {
            unsafe { resume_process(handle) };
        }
        self.update(id, |job| {
            job.state = JobState::Downloading;
            job.message = "Downloading...".to_string();
        });
        true
    }

    /// Stop a download, whether it is running or still queued.
    pub fn cancel(&self, id: &str) -> bool {
        {
            let mut queue = self.queue.lock().unwrap();
            let before = queue.len();
            queue.retain(|job| job.id != id);
            if queue.len() != before {
                self.update(id, |job| {
                    job.state = JobState::Cancelled;
                    job.message = "Cancelled".to_string();
                });
                return true;
            }
        }
        let mut running = match self.running.lock() {
            Ok(running) => running,
            Err(_) => return false,
        };
        let Some(live) = running.get_mut(id) else {
            return false;
        };
        live.cancelled = true;
        if let Some(handle) = live.handle {
            unsafe { resume_process(handle) };
        }
        self.update(id, |job| {
            job.state = JobState::Cancelled;
            job.message = "Cancelled".to_string();
        });
        true
    }

    /// Remove a finished download from the list.
    pub fn forget(&self, id: &str) -> bool {
        let mut jobs = self.jobs.lock().unwrap();
        let before = jobs.len();
        jobs.retain(|job| job.id != id);
        jobs.len() != before
    }

    /// Remove everything that is not actively downloading or paused: the
    /// queued jobs are dropped so they never start, and the finished,
    /// failed and cancelled entries are cleared from the list.
    pub fn clear_list(&self) -> usize {
        let removable: Vec<String> = {
            let jobs = self.jobs.lock().unwrap();
            jobs.iter()
                .filter(|job| !matches!(job.state, JobState::Downloading | JobState::Paused))
                .map(|job| job.id.clone())
                .collect()
        };
        if removable.is_empty() {
            return 0;
        }
        {
            let mut queue = self.queue.lock().unwrap();
            queue.retain(|request| !removable.contains(&request.id));
        }
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|job| !removable.contains(&job.id));
        removable.len()
    }

    /// Run the queue: several downloads at a time, in the order they were added.
    pub fn start_worker(self: &Arc<Self>) {
        for _ in 0..MAX_CONCURRENT_DOWNLOADS {
            let depot = Arc::clone(self);
            thread::spawn(move || loop {
                let next = {
                    let mut queue = depot.queue.lock().unwrap();
                    if queue.is_empty() {
                        None
                    } else {
                        Some(queue.remove(0))
                    }
                };
                let Some(request) = next else {
                    thread::sleep(Duration::from_millis(400));
                    continue;
                };
                depot.run_job(request);
            });
        }
    }

    fn run_job(&self, mut request: JobRequest) {
        eprintln!("queued {} ({})", request.id, request.title);
        self.update(&request.id, |job| {
            job.state = JobState::Downloading;
            job.progress = 0.0;
            job.message = "Connecting to Steam...".to_string();
        });

        // Experimental mode first; anything but a rejected key falls back to
        // the account path so a missing manifest never blocks a download.
        let outcome = if request.manifest_token.is_some() {
            match self.run_manifest(&request) {
                Outcome::FallbackWith(reason) => {
                    request.note = Some(reason);
                    self.run_depot(&request)
                }
                other => other,
            }
        } else {
            self.run_depot(&request)
        };

        {
            let mut running = self.running.lock().unwrap();
            running.remove(&request.id);
        }

        match outcome {
            Outcome::Done => {
                // Keep the item's details next to its files, so the library
                // can show a picture and the creator.
                if let Some(meta) = &request.meta {
                    crate::library::write_meta(&request.target_dir, meta);
                }
                self.update(&request.id, |job| {
                    job.state = JobState::Done;
                    job.progress = 100.0;
                    job.message = "Download complete".to_string();
                    job.path = Some(request.target_dir.to_string_lossy().to_string());
                })
            }
            Outcome::Cancelled => self.update(&request.id, |job| {
                job.state = JobState::Cancelled;
                job.message = "Cancelled".to_string();
            }),
            Outcome::Failed(message) => self.update(&request.id, |job| {
                job.state = JobState::Failed;
                job.message = message;
            }),
            Outcome::FallbackWith(message) => self.update(&request.id, |job| {
                job.state = JobState::Failed;
                job.message = message;
            }),
            Outcome::TokenRejected(message) => {
                // A dead key will fail every item, so stop the whole batch.
                self.update(&request.id, |job| {
                    job.state = JobState::Failed;
                    job.message = message.clone();
                    job.error_code = Some("TOKEN_REJECTED".to_string());
                });
                let dropped = {
                    let mut queue = self.queue.lock().unwrap();
                    queue.drain(..).collect::<Vec<_>>()
                };
                for pending in dropped {
                    self.update(&pending.id, |job| {
                        if job.state == JobState::Queued {
                            job.state = JobState::Failed;
                            job.message = "Stopped: ManifestHub rejected the API key.".to_string();
                            job.error_code = Some("TOKEN_REJECTED".to_string());
                        }
                    });
                }
            }
        }
    }
}

/// How a download attempt ended.
enum Outcome {
    Done,
    Failed(String),
    Cancelled,
    /// No usable manifest — try the Steam account, showing the reason.
    FallbackWith(String),
    /// A 403 from ManifestHub: stop this item and every queued item behind it.
    TokenRejected(String),
}

#[cfg(windows)]
mod process_control {
    use std::os::windows::io::AsRawHandle;

    #[link(name = "ntdll")]
    extern "system" {
        fn NtSuspendProcess(handle: isize) -> i32;
        fn NtResumeProcess(handle: isize) -> i32;
    }

    /// Suspend every thread of a process, so a download can be paused.
    pub unsafe fn suspend(handle: isize) {
        NtSuspendProcess(handle);
    }

    /// Resume a suspended process.
    pub unsafe fn resume(handle: isize) {
        NtResumeProcess(handle);
    }

    /// The raw handle of a running child.
    pub fn handle_of(child: &std::process::Child) -> Option<isize> {
        Some(child.as_raw_handle() as isize)
    }
}

#[cfg(not(windows))]
mod process_control {
    pub unsafe fn suspend(_handle: isize) {}
    pub unsafe fn resume(_handle: isize) {}
    pub fn handle_of(_child: &std::process::Child) -> Option<isize> {
        None
    }
}

use process_control::{handle_of, resume as resume_process, suspend as suspend_process};

impl Depot {
    /// Download with the signed-in Steam account.
    fn run_depot(&self, request: &JobRequest) -> Outcome {
        let args = vec![
            self.exe.to_string_lossy().to_string(),
            "-app".to_string(),
            APP_ID.to_string(),
            "-pubfile".to_string(),
            request.id.clone(),
            "-username".to_string(),
            request.account.clone(),
            "-remember-password".to_string(),
            "-verify-all".to_string(),
            "-dir".to_string(),
            request.target_dir.to_string_lossy().to_string(),
        ];
        self.update(&request.id, |job| {
            job.message = request
                .note
                .clone()
                .unwrap_or_else(|| "Connecting to Steam...".to_string());
        });
        self.run_tool(request, args, request.password.clone(), true)
    }

    /// Experimental path: resolve the item's depot manifest, fetch it from
    /// ManifestHub if it is not cached, then download with `-ugc` and the
    /// depot key file — no Steam sign-in involved.
    fn run_manifest(&self, request: &JobRequest) -> Outcome {
        use crate::manifest;

        self.update(&request.id, |job| {
            job.message = "Looking up the item on Steam...".to_string();
        });
        let client = match reqwest::blocking::Client::builder()
            .user_agent("WallForge/1.0 (ManifestHub)")
            .timeout(std::time::Duration::from_secs(30))
            .build()
        {
            Ok(client) => client,
            Err(err) => return Outcome::FallbackWith(format!("Could not start HTTP: {err}")),
        };
        let gid = match manifest::resolve_gid(&client, &request.id) {
            Ok(gid) => gid,
            Err(err) => return Outcome::FallbackWith(err),
        };

        let manifest_path = if let Some(cached) = manifest::cached_manifest(&request.target_dir, &gid) {
            self.update(&request.id, |job| {
                job.message = "Using the cached manifest...".to_string();
            });
            cached
        } else {
            let token = request.manifest_token.clone().unwrap_or_default();
            if token.trim().is_empty() {
                return Outcome::FallbackWith(
                    "No ManifestHub key saved - using the Steam account.".to_string(),
                );
            }
            let id = request.id.clone();
            let job_id = request.id.clone();
            let depot = self;
            self.update(&request.id, |job| {
                job.message = "Fetching the manifest from ManifestHub...".to_string();
            });
            let bytes = match manifest::fetch_manifest(&gid, &token, |message| {
                depot.update(&job_id, |job| job.message = message.to_string());
            }) {
                Ok(bytes) => bytes,
                Err(err) if err.code == "TOKEN_REJECTED" => {
                    return Outcome::TokenRejected(err.message)
                }
                Err(err) => {
                    return Outcome::FallbackWith(format!(
                        "ManifestHub: {} - using the Steam account.",
                        err.message
                    ))
                }
            };
            match manifest::stage_manifest(&id, &gid, &bytes, &request.target_dir) {
                Ok(path) => path,
                Err(err) => {
                    return Outcome::FallbackWith(format!("{err} - using the Steam account."))
                }
            }
        };

        let key_file = match manifest::ensure_depot_keys() {
            Ok(path) => path,
            Err(err) => return Outcome::FallbackWith(format!("{err} - using the Steam account.")),
        };
        self.update(&request.id, |job| {
            job.message = "Downloading with the manifest...".to_string();
        });
        let args = crate::manifest::depot_args(
            &self.exe,
            &gid,
            &key_file,
            &manifest_path,
            &request.target_dir,
        );
        self.run_tool(request, args, String::new(), false)
    }

    /// Run DepotDownloaderMod with the given arguments, tracking progress and
    /// pause/cancel. `password` is only used on the account path.
    fn run_tool(
        &self,
        request: &JobRequest,
        mut args: Vec<String>,
        mut password: String,
        allow_password_retry: bool,
    ) -> Outcome {
        let mut passed_password = false;

        loop {
            if passed_password {
                args.push("-password".to_string());
                args.push(password.clone());
            }

            let mut child = match depot_command(&args[0])
                .args(&args[1..])
                .current_dir(
                    self.exe
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| PathBuf::from(".")),
                )
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(err) => {
                    return Outcome::Failed(format!("Could not start the download tool: {err}"))
                }
            };

            let handle = handle_of(&child);
            {
                let mut running = self.running.lock().unwrap();
                running.insert(
                    request.id.clone(),
                    Running {
                        handle,
                        paused: false,
                        cancelled: false,
                    },
                );
            }

            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            let mut stdin = child.stdin.take();
            let (mut lines, mut errors) = match (stdout.map(pump), stderr.map(pump)) {
                (Some(lines), Some(errors)) => (lines, errors),
                (Some(lines), None) => (lines, mpsc::channel().1),
                (None, Some(errors)) => (mpsc::channel().1, errors),
                (None, None) => (mpsc::channel().1, mpsc::channel().1),
            };

            let mut buffer = String::new();
            let mut tail: Vec<String> = Vec::new();
            let mut answered_password = false;
            let mut needs_password = false;
            let mut cancelled = false;
            let mut paused = false;
            let deadline = Instant::now() + DOWNLOAD_TIMEOUT;

            'read: loop {
                let Some(chunk) = select_chunk(&mut lines, &mut errors) else {
                    break;
                };
                buffer.push_str(&chunk);

                // The password prompt arrives without a newline, so answer it
                // as soon as it shows up in the buffer.
                if !answered_password
                    && !password.is_empty()
                    && buffer.contains("Enter account password for")
                    && stdin.is_some()
                {
                    if let Some(stream) = stdin.as_mut() {
                        let _ = stream.write_all(format!("{password}\n").as_bytes());
                        let _ = stream.flush();
                        answered_password = true;
                        self.update(&request.id, |job| {
                            job.message = format!("Signing in as {}...", request.account);
                        });
                    }
                }

                for trimmed in drain_lines(&mut buffer) {
                    tail.push(trimmed.clone());
                    if tail.len() > 8 {
                        tail.remove(0);
                    }

                    if trimmed.contains("Unable to locate manifest ID") {
                        let _ = child.kill();
                        return if allow_password_retry {
                            Outcome::Failed(
                                "Steam could not find that item for this account. Subscribe to it first, or use an account that owns it."
                                    .to_string(),
                            )
                        } else {
                            Outcome::FallbackWith("Steam had no manifest for this item.".to_string())
                        };
                    }
                    if trimmed.contains("No valid depot key") {
                        let _ = child.kill();
                        return if allow_password_retry {
                            Outcome::Failed(
                                "That account does not own Wallpaper Engine, so it cannot download wallpapers."
                                    .to_string(),
                            )
                        } else {
                            Outcome::FallbackWith("The depot key was not accepted.".to_string())
                        };
                    }
                    if trimmed.contains("Access token was rejected") {
                        let _ = child.kill();
                        if !allow_password_retry {
                            return Outcome::FallbackWith(
                                "Steam rejected the anonymous session.".to_string(),
                            );
                        }
                        // The cached token is dead: sign in with the password.
                        if password.is_empty() {
                            return Outcome::Failed(
                                "Your Steam session expired. Sign in again from the Steam button."
                                    .to_string(),
                            );
                        }
                        needs_password = true;
                        break 'read;
                    }
                    if trimmed.starts_with("Unable to login to Steam3:")
                        || trimmed.starts_with("Failed to authenticate with Steam:")
                    {
                        let reason = trimmed.splitn(2, ':').nth(1).unwrap_or("").to_string();
                        let _ = child.kill();
                        return if allow_password_retry {
                            Outcome::Failed(login_error_text(&reason))
                        } else {
                            Outcome::FallbackWith(login_error_text(&reason))
                        };
                    }
                    if trimmed.contains("LogOn requires a username and password") {
                        let _ = child.kill();
                        return Outcome::Failed(format!(
                            "Steam refused the saved password for {}. Press Password next to that account to update it.",
                            request.account
                        ));
                    }
                    match parse_file_progress(&trimmed) {
                        // The tool prints cumulative depot progress at each
                        // file frontier, so keep the highest percentage seen.
                        Some((percent, file)) => self.update(&request.id, |job| {
                            job.progress = job.progress.max(percent).min(99.0);
                            job.message = format!("Downloading {file}");
                        }),
                        None => {
                            if let Some(message) = milestone_message(&trimmed) {
                                self.update(&request.id, |job| {
                                    job.message = message;
                                });
                            }
                        }
                    }
                }

                // Pause and cancel are handled between reads.
                {
                    let mut running = self.running.lock().unwrap();
                    if let Some(live) = running.get_mut(&request.id) {
                        if live.cancelled {
                            cancelled = true;
                        }
                        if live.paused && !paused {
                            paused = true;
                        } else if !live.paused && paused {
                            paused = false;
                        }
                    }
                }
                if cancelled {
                    let _ = child.kill();
                    return Outcome::Cancelled;
                }
                if paused {
                    self.update(&request.id, |job| {
                        job.state = JobState::Paused;
                        job.message = "Paused".to_string();
                    });
                    // Wait for the pause to be lifted.
                    loop {
                        thread::sleep(Duration::from_millis(200));
                        let mut running = self.running.lock().unwrap();
                        let Some(live) = running.get_mut(&request.id) else {
                            break;
                        };
                        if live.cancelled {
                            let _ = child.kill();
                            return Outcome::Cancelled;
                        }
                        if !live.paused {
                            break;
                        }
                    }
                    self.update(&request.id, |job| {
                        job.state = JobState::Downloading;
                        job.message = "Downloading...".to_string();
                    });
                }

                if Instant::now() > deadline {
                    let _ = child.kill();
                    return Outcome::Failed("Download timed out after 10 minutes".to_string());
                }
            }

            if needs_password {
                // Try again, this time with the password on the command line.
                passed_password = true;
                password.clear();
                continue;
            }

            if !buffer.trim().is_empty() {
                eprintln!("{}", buffer.trim_end());
                tail.push(buffer.trim_end().to_string());
            }

            match child.wait() {
                Ok(status) if status.success() => {
                    if tail.iter().any(|line| line.contains("Total downloaded: 0 B"))
                        && !folder_has_content(&request.target_dir)
                    {
                        return if allow_password_retry {
                            Outcome::Failed(
                                "Nothing was downloaded. The account may not own Wallpaper Engine, or it may not have subscribed to this item."
                                    .to_string(),
                            )
                        } else {
                            Outcome::FallbackWith(
                                "The manifest downloaded nothing - using the Steam account.".to_string(),
                            )
                        };
                    }
                    return Outcome::Done;
                }
                Ok(_) => {
                    let message = meaningful_error(&tail);
                    return if allow_password_retry {
                        Outcome::Failed(message)
                    } else {
                        Outcome::FallbackWith(format!("{message} - using the Steam account."))
                    };
                }
                Err(err) => {
                    return if allow_password_retry {
                        Outcome::Failed(format!("Download failed: {err}"))
                    } else {
                        Outcome::FallbackWith(format!("Download failed: {err} - using the Steam account."))
                    }
                }
            }
        }
    }
}

/// Whether a wallpaper folder already holds files.
///
/// The download tool reports nothing downloaded when every file is already
/// on disk, which is a successful re-download rather than a failure.
fn folder_has_content(folder: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .map(|name| name != "project.json")
            .unwrap_or(true)
    })
}

/// The line that explains the failure, rather than the last thing printed.
fn meaningful_error(tail: &[String]) -> String {
    const MARKERS: [&str; 8] = [
        "access token was rejected",
        "unable to login",
        "failed to authenticate",
        "no code was provided",
        "steam guard",
        "invalidpassword",
        "lost connection to steam",
        "error: ",
    ];
    for line in tail {
        let lowered = line.to_lowercase();
        if lowered.contains("lost connection to steam") {
            return "Steam dropped the connection — it may be rate-limiting this account. Wait a minute, then try again.".to_string();
        }
        if MARKERS.iter().any(|marker| lowered.contains(marker)) {
            return line.clone();
        }
    }
    tail.last()
        .cloned()
        .unwrap_or_else(|| "Download failed".to_string())
}

/// DepotDownloaderMod prints cumulative depot progress at each file it
/// finishes, e.g. ` 17.55% C:\...\preview.gif`. Parse the percentage and the
/// file being fetched.
pub fn parse_file_progress(line: &str) -> Option<(f64, String)> {
    let trimmed = line.trim();
    let (number, rest) = trimmed.split_once('%')?;
    let percent = number.trim().parse::<f64>().ok()?;
    if !(0.0..=100.0).contains(&percent) {
        return None;
    }
    let path = rest.trim();
    if path.is_empty() {
        return None;
    }
    let file = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_string();
    if file.is_empty() {
        return None;
    }
    Some((percent, file))
}

/// Short status lines for the steps of a download that carry no percentage.
fn milestone_message(line: &str) -> Option<String> {
    let lowered = line.to_lowercase();
    if lowered.contains("downloading depot") && lowered.contains("manifest") {
        return Some("Downloading the update list…".to_string());
    }
    if lowered.starts_with("got manifest") || lowered.starts_with("manifest ") {
        return Some("Update list verified".to_string());
    }
    if lowered.starts_with("pre-allocating") {
        return Some("Preparing files…".to_string());
    }
    if lowered.starts_with("connecting to steam") || lowered.contains("got depot key") {
        return Some("Connected to Steam".to_string());
    }
    if lowered.starts_with("depot ") && lowered.contains("downloaded") {
        return Some("Finalizing…".to_string());
    }
    None
}

/// The message for a failed authentication.
pub fn login_error_text(reason: &str) -> String {
    let reason = reason.trim().trim_end_matches('.').to_lowercase();
    if reason.contains("invalidpassword") {
        "Steam rejected that username or password.".to_string()
    } else if reason.contains("accountdisabled") {
        "That Steam account is disabled, so it cannot be used.".to_string()
    } else if reason.contains("ratelimitexceeded") {
        "Steam is rate-limiting sign-ins. Wait a few minutes and try again.".to_string()
    } else if reason.contains("needstwofactor") {
        "Steam wants your Steam Guard code.".to_string()
    } else if reason.contains("accountlogondenied") {
        "Steam emailed you a sign-in code.".to_string()
    } else if reason.contains("invalidloginauthcode") {
        "That Steam Guard code was not accepted.".to_string()
    } else if reason.contains("expired") {
        "That Steam session expired. Sign in again.".to_string()
    } else if reason.contains("revoked") {
        "Steam revoked that session. Sign in again.".to_string()
    } else if reason.contains("accessdenied") {
        "Steam denied the sign-in.".to_string()
    } else if reason.contains("serviceunavailable") {
        "Steam is unavailable right now. Try again in a minute.".to_string()
    } else if reason.is_empty() {
        "Steam sign-in failed.".to_string()
    } else {
        format!("Steam sign-in failed: {reason}")
    }
}

/// Read the sign-in prompt DepotDownloaderMod printed, if any.
pub fn login_prompt(tail: &str) -> String {
    if tail.contains("authentication code sent to your email")
        || tail.contains("auth code sent to the email")
    {
        return "Enter the Steam Guard code Steam emailed you.".to_string();
    }
    if tail.contains("Enter account password for") {
        return "Steam asked for the password again. Start the sign-in once more.".to_string();
    }
    String::new()
}

/// Turn the ASCII QR rows into an SVG document.
pub fn qr_svg(rows: &[String]) -> String {
    let rows: Vec<&String> = rows.iter().filter(|row| !row.is_empty()).collect();
    if rows.is_empty() {
        return String::new();
    }
    let width = rows[0].chars().count();
    let mut parts = String::new();
    parts.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {}\" shape-rendering=\"crispEdges\">",
        rows.len()
    ));
    parts.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#fff\"/>");
    for (row_index, row) in rows.iter().enumerate() {
        for (column, ch) in row.chars().enumerate() {
            if ch == ' ' {
                continue;
            }
            parts.push_str(&format!(
                "<rect x=\"{column}\" y=\"{row_index}\" width=\"1\" height=\"1\" fill=\"#000\"/>"
            ));
        }
    }
    parts.push_str("</svg>");
    parts
}

/// Arguments for a sign-in probe against an item that does not exist.
pub fn login_command(
    exe: &Path,
    method: &str,
    username: &str,
    password: &str,
    workdir: &Path,
) -> Vec<String> {
    let mut args = vec![
        exe.to_string_lossy().to_string(),
        "-app".to_string(),
        APP_ID.to_string(),
        "-pubfile".to_string(),
        "1".to_string(),
        "-dir".to_string(),
        workdir.to_string_lossy().to_string(),
        "-remember-password".to_string(),
    ];
    if method == "qr" {
        args.push("-qr".to_string());
    } else {
        args.push("-username".to_string());
        args.push(username.to_string());
        args.push("-password".to_string());
        args.push(password.to_string());
    }
    args
}

/// Start a Steam sign-in and watch its output.
pub fn start_login(
    depot: &Arc<Depot>,
    method: &str,
    username: &str,
    password: &str,
) -> Result<(), String> {
    if !depot.exe.is_file() {
        return Err("The download tool is missing, so Steam sign-in is unavailable.".to_string());
    }
    if method != "password" && method != "qr" {
        return Err("Unknown sign-in method.".to_string());
    }
    if method == "password" && (username.trim().is_empty() || password.is_empty()) {
        return Err("Enter your Steam username and password.".to_string());
    }

    let workdir = std::env::temp_dir().join(format!(
        "wallforge-login-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    std::fs::create_dir_all(&workdir).map_err(|err| err.to_string())?;

    let args = login_command(&depot.exe, method, username, password, &workdir);
    let mut child = depot_command(&args[0])
        .args(&args[1..])
        .current_dir(
            depot
                .exe
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(".")),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Could not start the download tool: {err}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    {
        let mut state = depot.login.lock().map_err(|_| "login state busy")?;
        state.status = "starting".to_string();
        state.message = "Connecting to Steam.".to_string();
        state.username = if username.is_empty() {
            None
        } else {
            Some(username.to_string())
        };
        state.qr.clear();
        state.qr_version += 1;
    }
    if let Ok(mut slot) = depot.login_stdin.lock() {
        *slot = child.stdin.take();
    }

    let depot = Arc::clone(depot);
    thread::spawn(move || {
        let mut child = child;
        {
            // The prompts arrive without a trailing newline, so read in chunks
            // and scan the buffer instead of waiting for a line.
            let mut buffer = String::new();
            let mut in_qr = false;
            let mut prompted = false;
            let (mut lines, mut errors) = match (stdout.map(pump), stderr.map(pump)) {
                (Some(lines), Some(errors)) => (lines, errors),
                (Some(lines), None) => (lines, mpsc::channel().1),
                (None, Some(errors)) => (mpsc::channel().1, errors),
                (None, None) => (mpsc::channel().1, mpsc::channel().1),
            };

            loop {
                let Some(chunk) = select_chunk(&mut lines, &mut errors) else {
                    break;
                };
                buffer.push_str(&chunk);

                if buffer.contains("Enter account password for") {
                    in_qr = false;
                }
                if buffer.contains("login to Steam") && buffer.contains("qr") {
                    in_qr = true;
                }
                if in_qr {
                    let rows: Vec<String> = buffer
                        .lines()
                        .filter(|line| !line.trim().is_empty())
                        .map(|line| line.to_string())
                        .collect();
                    if !rows.is_empty() {
                        if let Ok(mut state) = depot.login.lock() {
                            state.qr = rows;
                            state.qr_version += 1;
                        }
                    }
                    buffer.clear();
                    continue;
                }

                // Check the partial line too: prompts have no newline.
                if !buffer.contains(['\r', '\n']) && !buffer.trim().is_empty() {
                    handle_login_line(&depot, buffer.trim_end(), &mut prompted);
                }

                for trimmed in drain_lines(&mut buffer) {

                    if trimmed.contains("Next time you can login") {
                        let account = trimmed
                            .split("-username ")
                            .nth(1)
                            .and_then(|rest| rest.split_whitespace().next())
                            .unwrap_or_default()
                            .to_string();
                        if let Ok(mut state) = depot.login.lock() {
                            state.status = "success".to_string();
                            state.message = format!("Signed in as {account}.");
                            state.username = Some(account);
                        }
                        let _ = child.kill();
                        break;
                    }
                    if trimmed.contains("Access token was rejected") {
                        if let Ok(mut state) = depot.login.lock() {
                            state.status = "error".to_string();
                            state.message =
                                "That saved Steam session is no longer valid. Sign in again.".to_string();
                        }
                        let _ = child.kill();
                        break;
                    }
                    if trimmed.starts_with("Failed to authenticate with Steam:")
                        || trimmed.starts_with("Unable to login to Steam3:")
                    {
                        let reason = trimmed.splitn(2, ':').nth(1).unwrap_or("").to_string();
                        if let Ok(mut state) = depot.login.lock() {
                            state.status = "error".to_string();
                            state.message = login_error_text(&reason);
                        }
                        let _ = child.kill();
                        break;
                    }
                    if trimmed.contains("STEAM GUARD! Use the Steam Mobile App")
                        || trimmed.contains("This account is protected by Steam Guard")
                    {
                        if let Ok(mut state) = depot.login.lock() {
                            if state.status != "code" {
                                state.status = "confirm".to_string();
                                state.message = "Waiting for Steam Guard.".to_string();
                            }
                        }
                    }
                    let prompt = login_prompt(&trimmed);
                    if !prompt.is_empty() && !prompted {
                        prompted = true;
                        if let Ok(mut state) = depot.login.lock() {
                            state.status = "code".to_string();
                            state.message = prompt;
                        }
                    }
                }
            }
            // A prompt can arrive without a newline at the very end.
            if !buffer.trim().is_empty() {
                let trimmed = buffer.trim_end().to_string();
                handle_login_line(&depot, &trimmed, &mut prompted);
            }
        }
        let _ = child.wait();
        if let Ok(mut slot) = depot.login_stdin.lock() {
            *slot = None;
        }
        let _ = std::fs::remove_dir_all(&workdir);
    });

    Ok(())
}

/// React to what the sign-in printed: a Guard prompt, a success, or a
/// failure.
fn handle_login_line(depot: &Arc<Depot>, line: &str, prompted: &mut bool) {
    if line.contains("STEAM GUARD! Use the Steam Mobile App")
        || line.contains("This account is protected by Steam Guard")
    {
        if let Ok(mut state) = depot.login.lock() {
            if state.status != "code" {
                state.status = "confirm".to_string();
                state.message = "Waiting for Steam Guard.".to_string();
            }
        }
    }
    let prompt = login_prompt(line);
    if !prompt.is_empty() && !*prompted {
        *prompted = true;
        if let Ok(mut state) = depot.login.lock() {
            state.status = "code".to_string();
            state.message = prompt;
        }
    }
}

/// Send a Steam Guard code to the running sign-in.
pub fn submit_login_code(depot: &Arc<Depot>, code: &str) -> Result<(), String> {
    let code = code.trim();
    if code.is_empty() {
        return Err("Enter the Steam Guard code first.".to_string());
    }
    let mut slot = depot
        .login_stdin
        .lock()
        .map_err(|_| "The sign-in process is not accepting input.".to_string())?;
    let stdin = slot
        .as_mut()
        .ok_or_else(|| "No Steam sign-in is in progress.".to_string())?;
    stdin
        .write_all(format!("{code}\n").as_bytes())
        .map_err(|_| "The sign-in process is no longer accepting input.".to_string())?;
    stdin
        .flush()
        .map_err(|_| "The sign-in process is no longer accepting input.".to_string())?;
    if let Ok(mut state) = depot.login.lock() {
        state.message = "Code sent to Steam.".to_string();
    }
    Ok(())
}

/// Stop a running sign-in.
pub fn cancel_login(depot: &Arc<Depot>) {
    if let Ok(mut slot) = depot.login_stdin.lock() {
        *slot = None;
    }
    if let Ok(mut state) = depot.login.lock() {
        state.status = "idle".to_string();
        state.message = String::new();
        state.qr.clear();
    }
}

/// Pump a child stream into a channel, so stdout and stderr can be read
/// together. The tool prints its prompts on stderr.
fn pump<R: Read + Send + 'static>(stream: R) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel::<String>();
    thread::spawn(move || {
        let mut stream = stream;
        let mut chunk = [0u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    let text = String::from_utf8_lossy(&chunk[..count]).to_string();
                    if sender.send(text).is_err() {
                        break;
                    }
                }
            }
        }
    });
    receiver
}

/// Split out every complete line in buffer, treating both `\r` and `\n` as
/// terminators — the tool rewrites progress lines with carriage returns, so
/// splitting on newlines alone merges whole batches of output into one line.
/// Anything not yet terminated stays in the buffer for the next read.
fn drain_lines(buffer: &mut String) -> Vec<String> {
    let mut lines = Vec::new();
    let taken = std::mem::take(buffer);
    let Some(last) = taken.rfind(['\r', '\n']) else {
        *buffer = taken;
        return lines;
    };
    let (terminated, tail) = taken.split_at(last + 1);
    lines.extend(
        terminated
            .split(['\r', '\n'])
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string),
    );
    *buffer = tail.to_string();
    lines
}

/// Take the next piece of output from whichever stream speaks first.
fn select_chunk(
    lines: &mut mpsc::Receiver<String>,
    errors: &mut mpsc::Receiver<String>,
) -> Option<String> {
    use mpsc::TryRecvError;
    loop {
        match lines.try_recv() {
            Ok(text) => return Some(text),
            Err(TryRecvError::Disconnected) => return errors.try_recv().ok(),
            Err(TryRecvError::Empty) => {}
        }
        match errors.try_recv() {
            Ok(text) => return Some(text),
            Err(TryRecvError::Disconnected) => return lines.try_recv().ok(),
            Err(TryRecvError::Empty) => {}
        }
        match lines.recv_timeout(Duration::from_millis(100)) {
            Ok(text) => return Some(text),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Ok(text) = errors.try_recv() {
                    return Some(text);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return errors.recv().ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_progress_is_parsed() {
        assert_eq!(
            parse_file_progress(r" 17.55% C:\Users\x\preview.gif"),
            Some((17.55, "preview.gif".to_string()))
        );
        assert_eq!(
            parse_file_progress("100.00% C:/Users/x/wolf_final.mp4"),
            Some((100.0, "wolf_final.mp4".to_string()))
        );
        assert_eq!(parse_file_progress("Progress: 42.5%"), None);
        assert_eq!(parse_file_progress("101.5% too big"), None);
        assert_eq!(parse_file_progress("nothing here"), None);
    }

    #[test]
    fn drain_lines_splits_carriage_returns() {
        let mut buffer = String::from(" 00.01% a\r\n17.55% b\r");
        let lines = drain_lines(&mut buffer);
        assert_eq!(lines, vec!["00.01% a".to_string(), "17.55% b".to_string()]);
        assert_eq!(buffer, "");

        let mut buffer = String::from("partial");
        assert!(drain_lines(&mut buffer).is_empty());
        assert_eq!(buffer, "partial");

        let mut buffer = String::from("one\r\ntwo\nthree\nrest");
        let lines = drain_lines(&mut buffer);
        assert_eq!(lines, vec!["one".to_string(), "two".to_string(), "three".to_string()]);
        assert_eq!(buffer, "rest");
    }

    #[test]
    fn lost_connection_is_reported_as_steam_throttling() {
        let tail = vec![
            "Connecting to Steam3... Done!".to_string(),
            "Lost connection to Steam. Reconnecting".to_string(),
        ];
        assert!(meaningful_error(&tail).contains("rate-limiting"));
    }

    #[test]
    fn milestone_messages_are_found() {
        assert!(milestone_message("Downloading depot 431960 manifest").is_some());
        assert!(milestone_message("Pre-allocating C:\\x\\scene.pkg").is_some());
        assert!(milestone_message(" 00.01% C:\\x\\scene.pkg").is_none());
        assert!(milestone_message("nothing recognizable").is_none());
    }

    #[test]
    fn errors_are_mapped() {
        assert!(login_error_text("InvalidPassword").contains("rejected"));
        assert!(login_error_text("AccountDisabled").contains("disabled"));
        assert!(login_error_text("something else").contains("something else"));
    }

    #[test]
    fn qr_rows_become_svg() {
        let rows = vec!["██  ██".to_string(), "  ████".to_string()];
        let svg = qr_svg(&rows);
        assert!(svg.contains("<svg"));
        assert!(svg.contains("<rect"));
        assert_eq!(qr_svg(&[]), "");
    }

    #[test]
    fn a_folder_with_files_counts_as_downloaded() {
        let dir = std::env::temp_dir().join(format!("wallforge-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // An empty folder means nothing was fetched.
        assert!(!folder_has_content(&dir));

        // A folder holding only project.json is still empty of wallpapers.
        std::fs::write(dir.join("project.json"), "{}").unwrap();
        assert!(!folder_has_content(&dir));

        // Any other file means the wallpaper is there.
        std::fs::write(dir.join("preview.gif"), "x").unwrap();
        assert!(folder_has_content(&dir));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
