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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: String::new(),
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
}
