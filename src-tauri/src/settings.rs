use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub download_dir: String,
    /// Watch the clipboard for supported links.
    pub watch_clipboard: bool,
    /// Start downloading as soon as a link is detected (otherwise it is queued for a click).
    pub auto_download: bool,
    /// Passed to yt-dlp's `--cookies-from-browser` (e.g. "firefox"). Needed for some Instagram posts.
    pub cookies_browser: String,
    /// Show a system notification when a download finishes while the window is not focused.
    pub notify: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: String::new(),
            watch_clipboard: true,
            auto_download: true,
            cookies_browser: String::new(),
            notify: true,
        }
    }
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

fn default_download_dir(app: &AppHandle) -> String {
    app.path()
        .download_dir()
        .or_else(|_| app.path().home_dir())
        .map(|d| d.join("ClipDrop"))
        .unwrap_or_else(|_| PathBuf::from("ClipDrop"))
        .to_string_lossy()
        .into_owned()
}

impl Settings {
    pub fn load(app: &AppHandle) -> Self {
        let mut settings: Settings = settings_path(app)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if settings.download_dir.trim().is_empty() {
            settings.download_dir = default_download_dir(app);
        }
        settings
    }

    pub fn save(&self, app: &AppHandle) -> Result<(), String> {
        let path = settings_path(app).ok_or("no config directory available")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }
}
