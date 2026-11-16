//! The downloads list, saved as JSON in the app data dir so it survives a restart.

use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;
use tauri::{AppHandle, Manager};

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("history.json"))
}

/// The saved list, or empty if there is none or it can't be read.
pub fn load<T: DeserializeOwned>(app: &AppHandle) -> Vec<T> {
    let Some(path) = path(app) else { return Vec::new() };
    let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("[clipdrop] ignoring unreadable {}: {e}", path.display());
        Vec::new()
    })
}

/// Writes the list to a temp file and renames it into place, so a crash mid-write can't leave
/// a half-written file behind.
pub fn save<T: Serialize>(app: &AppHandle, items: &[T]) -> Result<(), String> {
    let path = path(app).ok_or("no app data directory available")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec(items).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}
