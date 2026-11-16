//! Self-update: checks the GitHub release feed for a newer ClipDrop and offers to install it.
//! Updates are verified against the public key in `tauri.conf.json` before they are run.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(15);
const CHECK_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);
const MAX_NOTES_CHARS: usize = 600;

/// Prevents overlapping checks (e.g. the tray item clicked during a background check).
static CHECKING: AtomicBool = AtomicBool::new(false);
/// Version the user chose "Later" for; background checks won't ask about it again this session.
static DECLINED: Mutex<Option<String>> = Mutex::new(None);

/// Checks shortly after launch, then periodically while the app stays open.
pub fn spawn_background_checks(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_DELAY);
        loop {
            tauri::async_runtime::block_on(check(&app, false));
            std::thread::sleep(CHECK_INTERVAL);
        }
    });
}

/// A user-requested check: always reports the outcome, including "up to date" and errors.
pub fn check_now(app: AppHandle) {
    std::thread::spawn(move || tauri::async_runtime::block_on(check(&app, true)));
}

async fn check(app: &AppHandle, manual: bool) {
    if CHECKING.swap(true, Ordering::SeqCst) {
        return;
    }
    let result = check_inner(app, manual).await;
    CHECKING.store(false, Ordering::SeqCst);

    if let Err(e) = result {
        eprintln!("[clipdrop] update check failed: {e}");
        if manual {
            app.dialog()
                .message(format!("Couldn't check for updates.\n\n{e}"))
                .title("ClipDrop")
                .kind(MessageDialogKind::Error)
                .blocking_show();
        }
    }
}

async fn check_inner(app: &AppHandle, manual: bool) -> Result<(), String> {
    let exiting = app.clone();
    let update = app
        .updater_builder()
        // On Windows the installer path ends in process::exit, skipping RunEvent::Exit, so stop
        // running downloads here. Replacing the hook drops the plugin's default, so keep its cleanup.
        .on_before_exit(move || {
            crate::stop_downloads_for_exit(&exiting);
            exiting.cleanup_before_exit();
        })
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;

    let Some(update) = update else {
        if manual {
            app.dialog()
                .message(format!(
                    "You're on the latest version ({}).",
                    app.package_info().version
                ))
                .title("ClipDrop")
                .blocking_show();
        }
        return Ok(());
    };

    if !manual && DECLINED.lock().unwrap().as_deref() == Some(update.version.as_str()) {
        return Ok(());
    }

    let mut msg = format!(
        "ClipDrop {} is available (you have {}).",
        update.version, update.current_version
    );
    if let Some(notes) = update.body.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        let notes: String = notes.chars().take(MAX_NOTES_CHARS).collect();
        msg.push_str("\n\n");
        msg.push_str(&notes);
    }
    if crate::downloads_active(app) {
        msg.push_str("\n\nDownloads in progress will be stopped.");
    }
    msg.push_str("\n\nInstall it now? ClipDrop will restart.");

    let install = app
        .dialog()
        .message(msg)
        .title("Update available")
        .buttons(MessageDialogButtons::OkCancelCustom("Install".into(), "Later".into()))
        .blocking_show();
    if !install {
        *DECLINED.lock().unwrap() = Some(update.version.clone());
        return Ok(());
    }

    // Verifies the signature, then runs the installer (which closes the app on Windows).
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| format!("update failed: {e}"))?;
    app.restart();
}
