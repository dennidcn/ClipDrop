//! Thin wrapper around the yt-dlp binary: locating/installing it and running downloads
//! with machine-readable progress output.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{OnceLock, mpsc};

use crate::links::Platform;
use crate::settings::Settings;

const EXE_NAME: &str = if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" };

fn release_asset() -> &'static str {
    if cfg!(windows) {
        "yt-dlp.exe"
    } else if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else if cfg!(target_arch = "aarch64") {
        "yt-dlp_linux_aarch64"
    } else {
        "yt-dlp_linux"
    }
}

/// A `Command` that never flashes a console window on Windows.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.env("PYTHONIOENCODING", "utf-8").env("PYTHONUTF8", "1");
    cmd
}

fn runs(program: &str, arg: &str) -> bool {
    command(program)
        .arg(arg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn has_ffmpeg() -> bool {
    static FFMPEG: OnceLock<bool> = OnceLock::new();
    *FFMPEG.get_or_init(|| runs("ffmpeg", "-version"))
}

/// Returns a usable yt-dlp: the app-managed copy, else one on PATH, else downloads it into `bin_dir`.
pub fn locate_or_install(bin_dir: &Path) -> Result<PathBuf, String> {
    let managed = bin_dir.join(EXE_NAME);
    if managed.is_file() {
        return Ok(managed);
    }
    if runs("yt-dlp", "--version") {
        return Ok(PathBuf::from("yt-dlp"));
    }

    std::fs::create_dir_all(bin_dir).map_err(|e| format!("cannot create {}: {e}", bin_dir.display()))?;
    let url = format!("https://github.com/yt-dlp/yt-dlp/releases/latest/download/{}", release_asset());
    let resp = ureq::get(&url).call().map_err(|e| format!("failed to download yt-dlp: {e}"))?;

    let tmp = bin_dir.join(format!("{EXE_NAME}.part"));
    {
        let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        std::io::copy(&mut resp.into_reader(), &mut file)
            .map_err(|e| format!("failed to download yt-dlp: {e}"))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &managed).map_err(|e| e.to_string())?;
    Ok(managed)
}

/// Runs `yt-dlp -U` and returns its output.
pub fn self_update(ytdlp: &Path) -> Result<String, String> {
    let out = command(ytdlp).arg("-U").output().map_err(|e| e.to_string())?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = text.trim().to_string();
    if out.status.success() { Ok(text) } else { Err(text) }
}

pub enum Event {
    Title(String),
    Progress { downloaded: f64, total: Option<f64>, speed: Option<f64> },
    File(String),
}

fn parse_num(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Downloads `url` into the configured directory. Returns the paths of the finished files.
pub fn download(
    ytdlp: &Path,
    url: &str,
    platform: Platform,
    settings: &Settings,
    mut on_event: impl FnMut(Event),
) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(&settings.download_dir)
        .map_err(|e| format!("cannot create download folder: {e}"))?;

    let mut cmd = command(ytdlp);
    cmd.args(["--newline", "--progress", "--no-simulate", "--no-mtime"])
        .args(["-P", &settings.download_dir])
        .args(["-o", &format!("{}_%(uploader_id,uploader|unknown).40s_%(id)s.%(ext)s", platform.slug())])
        .args(["--print", "before_dl:CDTITLE|%(title).120s"])
        .args(["--print", "after_move:CDFILE|%(filepath)s"])
        .args([
            "--progress-template",
            "download:CDPROG|%(progress.downloaded_bytes)s|%(progress.total_bytes,progress.total_bytes_estimate)s|%(progress.speed)s",
        ]);

    if has_ffmpeg() {
        cmd.args(["-f", "bv*[ext=mp4]+ba[ext=m4a]/bv*+ba/b", "--merge-output-format", "mp4"]);
    } else {
        // Without ffmpeg we can't merge separate audio/video streams, so stick to single-file formats.
        cmd.args(["-f", "b[ext=mp4]/b"]);
    }

    let cookies = settings.cookies_browser.trim();
    if !cookies.is_empty() {
        cmd.args(["--cookies-from-browser", cookies]);
    }

    cmd.arg("--").arg(url).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("failed to start yt-dlp: {e}"))?;

    // Read both pipes on their own threads and funnel the lines through one channel.
    let (tx, rx) = mpsc::channel::<String>();
    let readers: Vec<_> = [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|pipe| {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).split(b'\n').map_while(Result::ok) {
                let line = String::from_utf8_lossy(&line).trim_end().to_string();
                if tx.send(line).is_err() {
                    break;
                }
            }
        })
    })
    .collect();
    drop(tx);

    let mut files = Vec::new();
    let mut errors = Vec::new();
    for line in rx {
        if let Some(rest) = line.strip_prefix("CDPROG|") {
            let mut parts = rest.split('|');
            let downloaded = parts.next().and_then(parse_num).unwrap_or(0.0);
            let total = parts.next().and_then(parse_num);
            let speed = parts.next().and_then(parse_num);
            on_event(Event::Progress { downloaded, total, speed });
        } else if let Some(title) = line.strip_prefix("CDTITLE|") {
            on_event(Event::Title(title.to_string()));
        } else if let Some(file) = line.strip_prefix("CDFILE|") {
            files.push(file.to_string());
            on_event(Event::File(file.to_string()));
        } else if let Some(err) = line.strip_prefix("ERROR: ") {
            errors.push(err.to_string());
        }
    }
    for r in readers {
        let _ = r.join();
    }

    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() && !files.is_empty() {
        Ok(files)
    } else if let Some(err) = errors.pop() {
        Err(err)
    } else if status.success() {
        Err("yt-dlp finished but no video was found in this post".into())
    } else {
        Err(format!("yt-dlp exited with {status}"))
    }
}
