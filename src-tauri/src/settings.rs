use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub download_dir: String,
    /// Passed to yt-dlp's `--cookies-from-browser` (e.g. "firefox"). Needed for some Instagram posts.
    pub cookies_browser: String,
    /// Show a system notification when a download finishes while the window is not focused.
    pub notify: bool,
    /// Highest resolution saved from the Video (MP4) box.
    pub video_quality: VideoQuality,
    /// How many downloads run at the same time; the rest wait their turn.
    pub max_downloads: usize,
}

/// The Video quality setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoQuality {
    /// Up to 720p H.264: smaller files, for slow connections.
    #[serde(rename = "720")]
    P720,
    /// Up to 1080p H.264: plays everywhere (the default).
    #[serde(rename = "1080")]
    P1080,
    /// The highest resolution offered (4K on YouTube), in whatever codec it comes in.
    #[serde(rename = "best")]
    Best,
}

impl VideoQuality {
    /// The height cap, if any.
    pub fn max_height(self) -> Option<u32> {
        match self {
            VideoQuality::P720 => Some(720),
            VideoQuality::P1080 => Some(1080),
            VideoQuality::Best => None,
        }
    }
}

pub const MIN_DOWNLOADS: usize = 1;
pub const MAX_DOWNLOADS: usize = 5;

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: String::new(),
            cookies_browser: String::new(),
            notify: true,
            video_quality: VideoQuality::P1080,
            max_downloads: 3,
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

const ALLOWED_BROWSERS: &[&str] = &["", "firefox", "chrome", "edge", "brave"];

fn is_valid_download_dir(dir: &str) -> bool {
    let trimmed = dir.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return false;
    }
    // Reject Windows UNC / network share paths to prevent outbound SMB/NetNTLM coercion.
    if trimmed.starts_with(r"\\") || trimmed.starts_with("//") {
        return false;
    }
    std::path::Path::new(trimmed).is_absolute()
}

impl Settings {
    pub fn validate(&mut self) -> Result<(), String> {
        let browser = self.cookies_browser.trim().to_ascii_lowercase();
        if !ALLOWED_BROWSERS.contains(&browser.as_str()) {
            return Err("Unsupported cookies browser setting.".into());
        }
        self.cookies_browser = browser;

        let dir = self.download_dir.trim().to_string();
        if !is_valid_download_dir(&dir) {
            return Err("Download directory must be a valid local absolute path.".into());
        }
        self.download_dir = dir;

        if !(MIN_DOWNLOADS..=MAX_DOWNLOADS).contains(&self.max_downloads) {
            return Err(format!("Downloads at once must be {MIN_DOWNLOADS} to {MAX_DOWNLOADS}."));
        }
        Ok(())
    }

    pub fn load(app: &AppHandle) -> Self {
        let mut settings: Settings = settings_path(app)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let browser = settings.cookies_browser.trim().to_ascii_lowercase();
        settings.cookies_browser = if ALLOWED_BROWSERS.contains(&browser.as_str()) {
            browser
        } else {
            String::new()
        };

        if !is_valid_download_dir(&settings.download_dir) {
            settings.download_dir = default_download_dir(app);
        } else {
            settings.download_dir = settings.download_dir.trim().to_string();
        }
        settings.max_downloads = settings.max_downloads.clamp(MIN_DOWNLOADS, MAX_DOWNLOADS);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_cookies_browser_allowlist() {
        let mut s = Settings {
            download_dir: if cfg!(windows) { r"C:\Users\test\Downloads".into() } else { "/tmp/downloads".into() },
            cookies_browser: "firefox:/etc/passwd".into(),
            ..Default::default()
        };
        assert!(s.validate().is_err());

        s.cookies_browser = "Firefox".into();
        assert!(s.validate().is_ok());
        assert_eq!(s.cookies_browser, "firefox");
    }

    #[test]
    fn rejects_unc_and_relative_download_dirs() {
        assert!(!is_valid_download_dir(r"\\attacker.example\share"));
        assert!(!is_valid_download_dir("//attacker.example/share"));
        assert!(!is_valid_download_dir("relative/path"));
        assert!(!is_valid_download_dir(""));
    }

    #[test]
    fn reads_older_settings_and_checks_new_ones() {
        // A settings file from before these options existed gets the defaults.
        let old: Settings = serde_json::from_str(r#"{"notify":false}"#).unwrap();
        assert_eq!(old.video_quality, VideoQuality::P1080);
        assert_eq!(old.max_downloads, 3);
        let s: Settings = serde_json::from_str(r#"{"videoQuality":"best","maxDownloads":5}"#).unwrap();
        assert_eq!(s.video_quality, VideoQuality::Best);

        let mut s = Settings {
            download_dir: if cfg!(windows) { r"C:\Users\test\Downloads".into() } else { "/tmp/downloads".into() },
            max_downloads: 0,
            ..Default::default()
        };
        assert!(s.validate().is_err());
        s.max_downloads = 6;
        assert!(s.validate().is_err());
        s.max_downloads = 1;
        assert!(s.validate().is_ok());
    }
}
