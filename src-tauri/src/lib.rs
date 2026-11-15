mod downloader;
mod links;
mod settings;

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use links::{Link, Platform};
use settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum JobStatus {
    /// Seen on the clipboard while auto-download is off; waiting for the user.
    Detected,
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
    status: JobStatus,
    title: Option<String>,
    progress: f64,
    speed: Option<f64>,
    file: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum ToolStatus {
    Unknown,
    Installing,
    Ready,
    Error,
}

struct AppState {
    settings: Mutex<Settings>,
    jobs: Mutex<Vec<Job>>,
    next_id: AtomicU64,
    /// Resolved yt-dlp path. Held locked while installing so concurrent jobs wait for it.
    ytdlp: Mutex<Option<PathBuf>>,
    tool_status: Mutex<ToolStatus>,
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
        return Ok(path.clone());
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

/// Adds a job for `link` unless one for the same post already exists (and didn't fail).
fn add_job(app: &AppHandle, link: Link, start: bool) -> Job {
    let state = app.state::<AppState>();
    let (job, is_new) = {
        let mut jobs = state.jobs.lock().unwrap();
        if let Some(existing) = jobs.iter().find(|j| j.url == link.url && j.status != JobStatus::Error) {
            (existing.clone(), false)
        } else {
            jobs.retain(|j| j.url != link.url); // drop an old failed attempt
            let job = Job {
                id: state.next_id.fetch_add(1, Ordering::Relaxed),
                url: link.url,
                platform: link.platform,
                status: JobStatus::Detected,
                title: None,
                progress: 0.0,
                speed: None,
                file: None,
                error: None,
            };
            jobs.insert(0, job.clone());
            (job, true)
        }
    };
    if is_new {
        let _ = app.emit("job-updated", &job);
    }
    if start && job.status == JobStatus::Detected {
        start_job(app.clone(), job.id);
    }
    job
}

fn start_job(app: AppHandle, id: u64) {
    let Some(job) = update_job(&app, id, |j| {
        j.status = JobStatus::Downloading;
        j.progress = 0.0;
        j.speed = None;
        j.error = None;
    }) else {
        return;
    };

    std::thread::spawn(move || {
        let result = ensure_ytdlp(&app).and_then(|ytdlp| {
            let settings = app.state::<AppState>().settings.lock().unwrap().clone();
            let mut last_emit = Instant::now() - Duration::from_secs(1);
            downloader::download(&ytdlp, &job.url, job.platform, &settings, |event| match event {
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
                downloader::Event::File(file) => {
                    update_job(&app, id, |j| j.file = Some(file));
                }
            })
        });

        let finished = update_job(&app, id, |j| match &result {
            Ok(files) => {
                j.status = JobStatus::Done;
                j.progress = 1.0;
                j.speed = None;
                j.file = files.last().cloned();
            }
            Err(e) => {
                j.status = JobStatus::Error;
                j.speed = None;
                j.error = Some(e.clone());
            }
        });
        if let Some(job) = finished {
            notify(&app, &job);
        }
    });
}

fn notify(app: &AppHandle, job: &Job) {
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
            format!("{} video saved", job.platform.label()),
            job.title.clone().unwrap_or_else(|| job.url.clone()),
        ),
        _ => (
            format!("{} download failed", job.platform.label()),
            job.error.clone().unwrap_or_default(),
        ),
    };
    let _ = app.notification().builder().title(title).body(body).show();
}

fn spawn_clipboard_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut clipboard = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("clipboard unavailable: {e}");
                return;
            }
        };
        // Whatever is on the clipboard at launch is not a "new copy".
        let mut last = clipboard.get_text().unwrap_or_default();
        loop {
            std::thread::sleep(Duration::from_millis(600));
            let Ok(text) = clipboard.get_text() else { continue };
            if text == last {
                continue;
            }
            last = text;

            let (watch, auto) = {
                let s = app.state::<AppState>().settings.lock().unwrap().clone();
                (s.watch_clipboard, s.auto_download)
            };
            if !watch {
                continue;
            }
            for link in links::extract(&last) {
                add_job(&app, link, auto);
            }
        }
    });
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

fn job_file(state: &AppState, id: u64) -> Result<String, String> {
    state
        .jobs
        .lock()
        .unwrap()
        .iter()
        .find(|j| j.id == id)
        .and_then(|j| j.file.clone())
        .ok_or_else(|| "file not available".into())
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
    settings.save(&app)?;
    *state.settings.lock().unwrap() = settings.clone();
    Ok(settings)
}

#[tauri::command]
fn add_url(app: AppHandle, text: String) -> Result<Vec<Job>, String> {
    let found = links::extract(&text);
    if found.is_empty() {
        return Err("No X/Twitter, Instagram or TikTok post link found.".into());
    }
    Ok(found.into_iter().map(|link| add_job(&app, link, true)).collect())
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
    app.opener().open_path(file, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal_file(state: State<AppState>, id: u64) -> Result<(), String> {
    let file = job_file(&state, id)?;
    tauri_plugin_opener::reveal_item_in_dir(file).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_download_dir(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let dir = state.settings.lock().unwrap().download_dir.clone();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener().open_path(dir, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_ytdlp(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = ensure_ytdlp(&app)?;
        downloader::self_update(&path)
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
        .setup(|app| {
            let handle = app.handle().clone();
            let mica = apply_backdrop(app);
            eprintln!("[clipdrop] mica backdrop applied: {mica}");
            app.manage(AppState {
                settings: Mutex::new(Settings::load(&handle)),
                jobs: Mutex::new(Vec::new()),
                next_id: AtomicU64::new(1),
                ytdlp: Mutex::new(None),
                tool_status: Mutex::new(ToolStatus::Unknown),
                mica,
            });

            // Fetch yt-dlp in the background on first launch so the first download is quick.
            let h = handle.clone();
            std::thread::spawn(move || {
                let _ = ensure_ytdlp(&h);
            });

            spawn_clipboard_watcher(handle.clone());

            let show = MenuItem::with_id(app, "show", "Show ClipDrop", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("ClipDrop — watching clipboard")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "quit" => app.exit(0),
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
            // Closing the window keeps the app watching from the tray; quit from the tray menu.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
