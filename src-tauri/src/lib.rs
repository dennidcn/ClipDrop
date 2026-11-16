mod downloader;
mod links;
mod settings;
mod updater;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use downloader::Format;
use links::{Link, Platform};
use settings::Settings;

const MAX_CONCURRENT_DOWNLOADS: usize = 3;
const MAX_JOBS: usize = 200;

struct DownloadLimiter {
    active: Mutex<usize>,
    cvar: Condvar,
}

impl DownloadLimiter {
    fn new() -> Self {
        Self {
            active: Mutex::new(0),
            cvar: Condvar::new(),
        }
    }

    fn acquire(&self) -> DownloadPermit<'_> {
        let mut active = self.active.lock().unwrap();
        while *active >= MAX_CONCURRENT_DOWNLOADS {
            active = self.cvar.wait(active).unwrap();
        }
        *active += 1;
        DownloadPermit { limiter: self }
    }
}

struct DownloadPermit<'a> {
    limiter: &'a DownloadLimiter,
}

impl Drop for DownloadPermit<'_> {
    fn drop(&mut self) {
        let mut active = self.limiter.active.lock().unwrap();
        *active = active.saturating_sub(1);
        self.limiter.cvar.notify_one();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum JobStatus {
    Downloading,
    Done,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Job {
    id: u64,
    url: String,
    platform: Platform,
    /// MP4 (video) or MP3 (audio), from the box the link was pasted into.
    format: Format,
    status: JobStatus,
    title: Option<String>,
    progress: f64,
    speed: Option<f64>,
    /// The download finished and the audio is being converted to MP3.
    converting: bool,
    file: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum ToolStatus {
    Unknown,
    Installing,
    /// Fetching ffmpeg, needed once for MP3 conversion.
    #[serde(rename = "installing-converter")]
    InstallingConverter,
    Ready,
    Error,
}

struct AppState {
    settings: Mutex<Settings>,
    jobs: Mutex<Vec<Job>>,
    next_id: AtomicU64,
    /// Resolved yt-dlp path. Held locked while installing so concurrent jobs wait for it.
    ytdlp: Mutex<Option<PathBuf>>,
    /// ffmpeg, needed for MP3 conversion and used by video jobs when present.
    ffmpeg: ToolSlot,
    /// Deno, yt-dlp's JavaScript runtime for YouTube.
    deno: ToolSlot,
    /// Jobs currently resolving ffmpeg/Deno; the "getting ready" bar clears when this drops to 0.
    tool_setups: AtomicUsize,
    tool_status: Mutex<ToolStatus>,
    limiter: DownloadLimiter,
    /// Whether the Mica backdrop applied (Windows 11). The UI paints a solid background otherwise.
    mica: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    settings: Settings,
    jobs: Vec<Job>,
    tool_status: ToolStatus,
    mica: bool,
}

fn set_tool_status(app: &AppHandle, status: ToolStatus) {
    *app.state::<AppState>().tool_status.lock().unwrap() = status;
    let _ = app.emit("tool-status", status);
}

fn ensure_ytdlp(app: &AppHandle) -> Result<PathBuf, String> {
    let state = app.state::<AppState>();
    let mut slot = state.ytdlp.lock().unwrap();
    if let Some(path) = slot.as_ref() {
        if path.is_file() {
            return Ok(path.clone());
        }
    }
    let bin_dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?.join("bin");
    set_tool_status(app, ToolStatus::Installing);
    match downloader::locate_or_install(&bin_dir) {
        Ok(path) => {
            *slot = Some(path.clone());
            set_tool_status(app, ToolStatus::Ready);
            Ok(path)
        }
        Err(e) => {
            set_tool_status(app, ToolStatus::Error);
            Err(e)
        }
    }
}

/// A helper tool (ffmpeg, Deno) that is found on the machine or installed on first use.
#[derive(Default)]
struct ToolSlot {
    /// Held locked while installing so concurrent jobs that need the tool wait for it.
    state: Mutex<ToolState>,
    /// Set during an install, so jobs that only use the tool opportunistically skip it instead of waiting.
    installing: AtomicBool,
}

#[derive(Default)]
struct ToolState {
    path: Option<PathBuf>,
    /// The last failed install, so jobs queued behind it fail at once instead of each retrying.
    failure: Option<(Instant, String)>,
}

/// After a failed ffmpeg install, how long before another job may try again (e.g. the retry button).
const FFMPEG_RETRY_AFTER: Duration = Duration::from_secs(30);

/// Returns ffmpeg if one is available. When `install` is set (MP3 jobs) and none is found,
/// downloads the app-managed copy; video jobs just use ffmpeg when it happens to be there.
fn ensure_ffmpeg(app: &AppHandle, install: bool) -> Result<Option<PathBuf>, String> {
    let tool = &app.state::<AppState>().ffmpeg;
    ensure_tool(app, tool, install, Some(FFMPEG_RETRY_AFTER), downloader::find_ffmpeg, downloader::install_ffmpeg)
        .map_err(|e| format!("Couldn't get the MP3 converter (ffmpeg): {e}"))
}

/// Returns Deno for yt-dlp's YouTube JavaScript, installing it on first use. Deno is optional
/// (yt-dlp still works without it, with fewer formats), so a failed install doesn't fail the job,
/// and isn't retried until the next launch.
fn ensure_deno(app: &AppHandle) -> Option<PathBuf> {
    let tool = &app.state::<AppState>().deno;
    ensure_tool(app, tool, true, None, downloader::find_deno, downloader::install_deno)
        .inspect_err(|e| eprintln!("[clipdrop] couldn't install Deno, continuing without it: {e}"))
        .ok()
        .flatten()
}

/// Finds a helper tool, or with `install` downloads the app-managed copy. After a failed install,
/// further installs are refused until `retry_after` has passed (`None`: for the rest of the session).
fn ensure_tool(
    app: &AppHandle,
    tool: &ToolSlot,
    install: bool,
    retry_after: Option<Duration>,
    find: fn(&std::path::Path) -> Option<PathBuf>,
    installer: fn(&std::path::Path) -> Result<PathBuf, String>,
) -> Result<Option<PathBuf>, String> {
    if !install && tool.installing.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let mut state = tool.state.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(path) = state.path.as_ref().filter(|p| p.is_file()) {
        return Ok(Some(path.clone()));
    }
    let bin_dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?.join("bin");
    if let Some(found) = find(&bin_dir) {
        state.path = Some(found.clone());
        return Ok(Some(found));
    }
    if !install {
        return Ok(None);
    }
    if let Some((when, error)) = &state.failure
        && retry_after.is_none_or(|wait| when.elapsed() < wait)
    {
        return Err(error.clone());
    }

    tool.installing.store(true, Ordering::SeqCst);
    // Cleared by the job's ToolSetup guard once all of its tools are resolved, so it doesn't flicker.
    set_tool_status(app, ToolStatus::InstallingConverter);
    let result = installer(&bin_dir);
    tool.installing.store(false, Ordering::SeqCst);
    match result {
        Ok(path) => {
            state.path = Some(path.clone());
            state.failure = None;
            Ok(Some(path))
        }
        Err(e) => {
            state.failure = Some((Instant::now(), e.clone()));
            Err(e)
        }
    }
}

/// Counts a job resolving its helper tools; the last one to finish clears the "getting ready" bar.
struct ToolSetup<'a>(&'a AppHandle);

impl<'a> ToolSetup<'a> {
    fn begin(app: &'a AppHandle) -> Self {
        app.state::<AppState>().tool_setups.fetch_add(1, Ordering::SeqCst);
        Self(app)
    }
}

impl Drop for ToolSetup<'_> {
    fn drop(&mut self) {
        let state = self.0.state::<AppState>();
        let was_last = state.tool_setups.fetch_sub(1, Ordering::SeqCst) == 1;
        let installing = *state.tool_status.lock().unwrap() == ToolStatus::InstallingConverter;
        if was_last && installing {
            // yt-dlp is already installed whenever helper tools are resolved, so the engine is ready.
            set_tool_status(self.0, ToolStatus::Ready);
        }
    }
}

fn update_job(app: &AppHandle, id: u64, f: impl FnOnce(&mut Job)) -> Option<Job> {
    let state = app.state::<AppState>();
    let job = {
        let mut jobs = state.jobs.lock().unwrap();
        let job = jobs.iter_mut().find(|j| j.id == id)?;
        f(job);
        job.clone()
    };
    let _ = app.emit("job-updated", &job);
    Some(job)
}

/// Adds and starts a job for `link` unless one for the same post and format already exists
/// (and didn't fail). The same post can be saved as both MP4 and MP3.
fn add_job(app: &AppHandle, link: Link, format: Format) -> Job {
    let state = app.state::<AppState>();
    let (job, is_new) = {
        let mut jobs = state.jobs.lock().unwrap();
        let same = |j: &Job| j.url == link.url && j.format == format;
        if let Some(existing) = jobs.iter().find(|j| same(j) && j.status != JobStatus::Error) {
            (existing.clone(), false)
        } else {
            jobs.retain(|j| !same(j)); // drop an old failed attempt
            if jobs.len() >= MAX_JOBS {
                if let Some(pos) = jobs.iter().rposition(|j| j.status != JobStatus::Downloading) {
                    jobs.remove(pos);
                }
            }
            let job = Job {
                id: state.next_id.fetch_add(1, Ordering::Relaxed),
                url: link.url,
                platform: link.platform,
                format,
                status: JobStatus::Downloading,
                title: None,
                progress: 0.0,
                speed: None,
                converting: false,
                file: None,
                error: None,
            };
            jobs.insert(0, job.clone());
            (job, true)
        }
    };
    if is_new {
        let _ = app.emit("job-updated", &job);
        spawn_download(app.clone(), job.clone());
    }
    job
}

/// Restarts a finished or failed job (the retry button).
fn start_job(app: AppHandle, id: u64) {
    let job = {
        let state = app.state::<AppState>();
        let mut jobs = state.jobs.lock().unwrap();
        let Some(j) = jobs.iter_mut().find(|j| j.id == id) else {
            return;
        };
        if j.status == JobStatus::Downloading {
            return;
        }
        j.status = JobStatus::Downloading;
        j.progress = 0.0;
        j.speed = None;
        j.converting = false;
        j.error = None;
        j.clone()
    };
    let _ = app.emit("job-updated", &job);
    spawn_download(app, job);
}

/// Runs the download for `job` (already marked downloading) on a worker thread.
fn spawn_download(app: AppHandle, job: Job) {
    let id = job.id;
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        // Fetch the tools before taking a permit, so a one-off 200 MB ffmpeg install
        // doesn't use up download slots that other links could be using.
        let tools = ensure_ytdlp(&app).and_then(|ytdlp| {
            let needs_ffmpeg = job.format.needs_ffmpeg(job.platform);
            let needs_deno = downloader::needs_js_runtime(job.platform);
            let _setup = (needs_ffmpeg || needs_deno).then(|| ToolSetup::begin(&app));
            let ffmpeg = ensure_ffmpeg(&app, needs_ffmpeg)?;
            let deno = if needs_deno { ensure_deno(&app) } else { None };
            Ok((ytdlp, ffmpeg, deno))
        });
        let _permit = state.limiter.acquire();

        let result = tools.and_then(|(ytdlp, ffmpeg, deno)| {
            let settings = state.settings.lock().unwrap().clone();
            let mut last_emit = Instant::now() - Duration::from_secs(1);
            let on_event = |event| match event {
                downloader::Event::Title(title) => {
                    update_job(&app, id, |j| j.title = Some(title));
                }
                downloader::Event::Progress { downloaded, total, speed } => {
                    // yt-dlp prints progress very often; throttle UI updates.
                    if last_emit.elapsed() < Duration::from_millis(150) {
                        return;
                    }
                    last_emit = Instant::now();
                    update_job(&app, id, |j| {
                        if let Some(total) = total.filter(|t| *t > 0.0) {
                            j.progress = (downloaded / total).clamp(0.0, 1.0);
                        }
                        j.speed = speed;
                    });
                }
                downloader::Event::Converting => {
                    update_job(&app, id, |j| {
                        j.converting = true;
                        j.progress = 1.0;
                        j.speed = None;
                    });
                }
                downloader::Event::File(file) => {
                    update_job(&app, id, |j| j.file = Some(file));
                }
            };
            downloader::download(&ytdlp, &job.url, job.platform, job.format, &settings, ffmpeg.as_deref(), deno.as_deref(), on_event)
        });

        let finished = update_job(&app, id, |j| match &result {
            Ok(files) => {
                j.status = JobStatus::Done;
                j.progress = 1.0;
                j.speed = None;
                j.converting = false;
                j.file = files.last().cloned();
            }
            Err(e) => {
                j.status = JobStatus::Error;
                j.speed = None;
                j.converting = false;
                j.error = Some(e.clone());
            }
        });
        if let Some(job) = finished {
            notify(&app, &job);
        }
    });
}

fn notify(app: &AppHandle, job: &Job) {
    if QUITTING.load(Ordering::SeqCst) {
        return;
    }
    if !app.state::<AppState>().settings.lock().unwrap().notify {
        return;
    }
    let focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    if focused {
        return;
    }
    let (title, body) = match job.status {
        JobStatus::Done => (
            if job.format == Format::Mp3 {
                format!("{} MP3 saved", job.platform.label())
            } else {
                format!("{} video saved", job.platform.label())
            },
            job.title.clone().unwrap_or_else(|| job.url.clone()),
        ),
        _ => (
            format!("{} download failed", job.platform.label()),
            job.error.clone().unwrap_or_default(),
        ),
    };
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Applies the Windows 11 Mica backdrop to the main window. Returns false where it is unsupported.
fn apply_backdrop(app: &tauri::App) -> bool {
    #[cfg(windows)]
    if let Some(window) = app.get_webview_window("main") {
        return window_vibrancy::apply_mica(&window, None).is_ok();
    }
    let _ = app;
    false
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Whether any download is running (used to warn before an update restarts the app).
pub(crate) fn downloads_active(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let jobs = state.jobs.lock().unwrap();
    jobs.iter().any(|j| j.status == JobStatus::Downloading)
}

fn job_file(state: &AppState, id: u64) -> Result<PathBuf, String> {
    let file = state
        .jobs
        .lock()
        .unwrap()
        .iter()
        .find(|j| j.id == id)
        .and_then(|j| j.file.clone())
        .ok_or_else(|| "file not available".to_string())?;
    let download_dir = state.settings.lock().unwrap().download_dir.clone();
    downloader::validate_downloaded_file(&file, &download_dir)
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_snapshot(state: State<AppState>) -> Snapshot {
    Snapshot {
        settings: state.settings.lock().unwrap().clone(),
        jobs: state.jobs.lock().unwrap().clone(),
        tool_status: *state.tool_status.lock().unwrap(),
        mica: state.mica,
    }
}

#[tauri::command]
fn save_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> Result<Settings, String> {
    let mut settings = settings;
    if settings.download_dir.trim().is_empty() {
        settings.download_dir = state.settings.lock().unwrap().download_dir.clone();
    }
    settings.validate()?;
    settings.save(&app)?;
    *state.settings.lock().unwrap() = settings.clone();
    Ok(settings)
}

#[tauri::command]
fn add_url(app: AppHandle, text: String, format: Format) -> Result<Vec<Job>, String> {
    let found = links::extract(&text);
    if found.is_empty() {
        return Err("No X/Twitter, Instagram, TikTok or YouTube link found.".into());
    }
    Ok(found.into_iter().map(|link| add_job(&app, link, format)).collect())
}

#[tauri::command]
fn start_download(app: AppHandle, id: u64) {
    start_job(app, id);
}

#[tauri::command]
fn remove_job(state: State<AppState>, id: u64) {
    state
        .jobs
        .lock()
        .unwrap()
        .retain(|j| j.id != id || j.status == JobStatus::Downloading);
}

#[tauri::command]
fn clear_finished(state: State<AppState>) -> Vec<Job> {
    let mut jobs = state.jobs.lock().unwrap();
    jobs.retain(|j| j.status == JobStatus::Downloading);
    jobs.clone()
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app.dialog().file().blocking_pick_folder()?;
    picked.into_path().ok().map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
fn open_file(app: AppHandle, state: State<AppState>, id: u64) -> Result<(), String> {
    let file = job_file(&state, id)?;
    app.opener()
        .open_path(file.to_string_lossy().as_ref(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal_file(state: State<AppState>, id: u64) -> Result<(), String> {
    let file = job_file(&state, id)?;
    tauri_plugin_opener::reveal_item_in_dir(file).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_download_dir(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let mut settings = state.settings.lock().unwrap().clone();
    settings.validate()?;
    std::fs::create_dir_all(&settings.download_dir).map_err(|e| e.to_string())?;
    let canon_dir = std::fs::canonicalize(&settings.download_dir).map_err(|e| e.to_string())?;
    if !canon_dir.is_dir() {
        return Err("Download location is not a directory.".into());
    }
    app.opener()
        .open_path(canon_dir.to_string_lossy().as_ref(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_ytdlp(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = ensure_ytdlp(&app)?;
        let bin_dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?.join("bin");
        let (updated_path, msg) = downloader::self_update(&path, &bin_dir)?;
        *app.state::<AppState>().ytdlp.lock().unwrap() = Some(updated_path);
        Ok(msg)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();
            let mica = apply_backdrop(app);
            eprintln!("[clipdrop] mica backdrop applied: {mica}");
            app.manage(AppState {
                settings: Mutex::new(Settings::load(&handle)),
                jobs: Mutex::new(Vec::new()),
                next_id: AtomicU64::new(1),
                ytdlp: Mutex::new(None),
                ffmpeg: ToolSlot::default(),
                deno: ToolSlot::default(),
                tool_setups: AtomicUsize::new(0),
                tool_status: Mutex::new(ToolStatus::Unknown),
                limiter: DownloadLimiter::new(),
                mica,
            });

            // Fetch yt-dlp in the background on first launch so the first download is quick.
            let h = handle.clone();
            std::thread::spawn(move || {
                let _ = ensure_ytdlp(&h);
            });

            updater::spawn_background_checks(handle.clone());

            let show = MenuItem::with_id(app, "show", "Show ClipDrop", true, None::<&str>)?;
            let check_updates =
                MenuItem::with_id(app, "check_updates", "Check for updates", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &check_updates, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("ClipDrop")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "check_updates" => updater::check_now(app.clone()),
                    "quit" => request_quit(app.clone()),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window quits the app (the tray icon would otherwise keep it alive).
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                request_quit(window.app_handle().clone());
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            save_settings,
            add_url,
            start_download,
            remove_job,
            clear_finished,
            pick_folder,
            open_file,
            reveal_file,
            open_download_dir,
            update_ytdlp,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // Covers every way out (window close, tray Quit, update restart): don't leave yt-dlp running hidden.
            if let tauri::RunEvent::Exit = event {
                stop_downloads_for_exit();
            }
        });
}

/// Set once the app is exiting, so downloads it stops aren't reported as failures.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// Stops running downloads as the app exits (normal quit or an update install).
pub(crate) fn stop_downloads_for_exit() {
    QUITTING.store(true, Ordering::SeqCst);
    downloader::kill_all();
}

/// Quits, first asking for confirmation if downloads are still running.
fn request_quit(app: AppHandle) {
    if !downloads_active(&app) {
        app.exit(0);
        return;
    }
    // Clicking close or tray Quit again while the prompt is up shouldn't stack a second one.
    static PROMPT_OPEN: AtomicBool = AtomicBool::new(false);
    if PROMPT_OPEN.swap(true, Ordering::SeqCst) {
        return;
    }
    // The dialog blocks, so it can't run on the event-loop thread.
    std::thread::spawn(move || {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
        let mut dialog = app
            .dialog()
            .message("Downloads are still running. Quitting stops them.")
            .title("Quit ClipDrop?")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom("Quit".into(), "Keep downloading".into()));
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.parent(&window);
        }
        let quit = dialog.blocking_show();
        PROMPT_OPEN.store(false, Ordering::SeqCst);
        if quit {
            app.exit(0);
        }
    });
}
