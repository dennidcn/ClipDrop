//! Thin wrapper around the yt-dlp binary: locating/installing it and running downloads
//! with machine-readable progress output.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{OnceLock, mpsc};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::links::Platform;
use crate::settings::Settings;

const EXE_NAME: &str = if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" };
const FFMPEG_EXE_NAME: &str = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };

const MAX_CHECKSUM_BYTES: u64 = 128 * 1024;
const MAX_BINARY_BYTES: u64 = 100 * 1024 * 1024;
const ALLOWED_MEDIA_EXTENSIONS: &[&str] = &["mp4", "webm", "mkv", "mov", "m4a"];

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

fn runs(program: &Path, arg: &str) -> bool {
    if !program.is_absolute() {
        return false;
    }
    command(program)
        .arg(arg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Resolves `exe_name` strictly across absolute directories in `PATH` (never CWD).
fn find_in_path(exe_name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        if !dir.is_absolute() {
            continue;
        }
        let candidate = dir.join(exe_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn ffmpeg_path() -> Option<&'static Path> {
    static FFMPEG: OnceLock<Option<PathBuf>> = OnceLock::new();
    FFMPEG
        .get_or_init(|| {
            let candidate = find_in_path(FFMPEG_EXE_NAME)?;
            runs(&candidate, "-version").then_some(candidate)
        })
        .as_deref()
}

fn parse_sha256sum(sums_text: &str, asset_name: &str) -> Option<String> {
    for line in sums_text.lines() {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        if name == asset_name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Returns a usable yt-dlp: the app-managed copy, else downloads and verifies it into `bin_dir`
/// (with a fallback to an absolute path on `PATH` only if already verified to run).
pub fn locate_or_install(bin_dir: &Path) -> Result<PathBuf, String> {
    let managed = bin_dir.join(EXE_NAME);
    if managed.is_file() {
        return Ok(managed);
    }
    if let Some(sys_ytdlp) = find_in_path(EXE_NAME) {
        if runs(&sys_ytdlp, "--version") {
            return Ok(sys_ytdlp);
        }
    }

    install_managed(bin_dir)
}

pub fn install_managed(bin_dir: &Path) -> Result<PathBuf, String> {
    let managed = bin_dir.join(EXE_NAME);
    std::fs::create_dir_all(bin_dir).map_err(|e| format!("cannot create {}: {e}", bin_dir.display()))?;

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(120))
        .build();

    let asset = release_asset();
    let sums_url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";
    let sums_resp = agent
        .get(sums_url)
        .call()
        .map_err(|e| format!("failed to fetch yt-dlp checksums: {e}"))?;
    let mut sums_text = String::new();
    sums_resp
        .into_reader()
        .take(MAX_CHECKSUM_BYTES)
        .read_to_string(&mut sums_text)
        .map_err(|e| format!("failed to read yt-dlp checksums: {e}"))?;
    let expected_sha256 = parse_sha256sum(&sums_text, asset)
        .ok_or_else(|| format!("SHA-256 checksum for {asset} not found in release SHA2-256SUMS"))?;

    let url = format!("https://github.com/yt-dlp/yt-dlp/releases/latest/download/{asset}");
    let resp = agent
        .get(&url)
        .call()
        .map_err(|e| format!("failed to download yt-dlp: {e}"))?;

    let tmp = bin_dir.join(format!("{EXE_NAME}.part"));
    let actual_sha256 = {
        let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut reader = resp.into_reader().take(MAX_BINARY_BYTES + 1);
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 32 * 1024];
        let mut total: u64 = 0;
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| format!("failed to download yt-dlp: {e}"))?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > MAX_BINARY_BYTES {
                let _ = std::fs::remove_file(&tmp);
                return Err("yt-dlp download exceeded maximum allowed size".into());
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])
                .map_err(|e| format!("failed to write yt-dlp: {e}"))?;
        }
        file.flush().map_err(|e| e.to_string())?;
        hex_lower(&hasher.finalize())
    };

    if actual_sha256 != expected_sha256 {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "yt-dlp SHA-256 verification failed (expected {expected_sha256}, got {actual_sha256})"
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &managed).map_err(|e| e.to_string())?;
    Ok(managed)
}

/// Runs `yt-dlp -U` on the app-managed binary (or installs the managed binary if currently using a system binary).
pub fn self_update(ytdlp: &Path, bin_dir: &Path) -> Result<(PathBuf, String), String> {
    if !ytdlp.starts_with(bin_dir) {
        let managed = install_managed(bin_dir)?;
        return Ok((managed, "Installed latest verified yt-dlp.".into()));
    }
    let out = command(ytdlp).arg("-U").output().map_err(|e| e.to_string())?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = text.trim().to_string();
    if out.status.success() {
        Ok((ytdlp.to_path_buf(), text))
    } else {
        Err(text)
    }
}

pub enum Event {
    Title(String),
    Progress { downloaded: f64, total: Option<f64>, speed: Option<f64> },
    File(String),
}

fn parse_num(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

fn decode_printed_value(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed == "null" || trimmed == "NA" {
        return None;
    }
    let decoded = if trimmed.starts_with('"') {
        serde_json::from_str::<String>(trimmed).ok()?
    } else {
        trimmed.to_string()
    };
    let clean: String = decoded
        .chars()
        .map(|c| if c == '\n' || c == '\r' || c.is_control() { ' ' } else { c })
        .collect();
    let clean = clean.trim().to_string();
    (!clean.is_empty()).then_some(clean)
}

/// Verifies that `file` exists as a regular file inside `download_dir` and has an allowed media extension.
pub fn validate_downloaded_file(file: &str, download_dir: &str) -> Result<PathBuf, String> {
    let canon_dir = std::fs::canonicalize(download_dir)
        .map_err(|_| "Download folder does not exist.".to_string())?;
    let canon_file = std::fs::canonicalize(file)
        .map_err(|_| "Downloaded file does not exist.".to_string())?;

    if !canon_file.starts_with(&canon_dir) {
        return Err("Refusing to access file outside the configured download folder.".into());
    }
    if !canon_file.is_file() {
        return Err("Target path is not a regular file.".into());
    }
    let ext = canon_file
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .ok_or_else(|| "Downloaded file has no file extension.".to_string())?;
    if !ALLOWED_MEDIA_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("Refusing to open unsupported file extension '.{ext}'."));
    }
    Ok(canon_file)
}

enum PipeLine {
    Stdout(String),
    Stderr(String),
}

/// Downloads `url` into the configured directory. Returns the paths of the finished files.
pub fn download(
    ytdlp: &Path,
    url: &str,
    platform: Platform,
    settings: &Settings,
    mut on_event: impl FnMut(Event),
) -> Result<Vec<String>, String> {
    let mut settings = settings.clone();
    settings.validate()?;

    std::fs::create_dir_all(&settings.download_dir)
        .map_err(|e| format!("cannot create download folder: {e}"))?;

    let mut cmd = command(ytdlp);
    cmd.args([
        "--newline",
        "--progress",
        "--no-simulate",
        "--no-mtime",
        "--no-playlist",
        "--max-downloads",
        "10",
        "--max-filesize",
        "2G",
        "--socket-timeout",
        "30",
        "--allowed-extractors",
        platform.allowed_extractors(),
    ])
    .args(["-P", &format!("home:{}", settings.download_dir)])
    .args([
        "-o",
        &format!("{}_%(uploader_id,uploader|unknown).40s_%(id)s.%(ext)s", platform.slug()),
    ])
    // Use JSON formatting (%()j) so embedded newlines in post titles are escaped and cannot spoof CDFILE| lines.
    .args(["--print", "before_dl:CDTITLE|%(title.0:120)j"])
    .args(["--print", "after_move:CDFILE|%(filepath)j"])
    .args([
        "--progress-template",
        "download:CDPROG|%(progress.downloaded_bytes)s|%(progress.total_bytes,progress.total_bytes_estimate)s|%(progress.speed)s",
    ]);

    if let Some(ffmpeg) = ffmpeg_path() {
        cmd.arg("--ffmpeg-location")
            .arg(ffmpeg)
            .args(["-f", "bv*[ext=mp4]+ba[ext=m4a]/bv*+ba/b", "--merge-output-format", "mp4"]);
    } else {
        // Without ffmpeg we can't merge separate audio/video streams, so stick to single-file formats.
        cmd.args(["-f", "b[ext=mp4]/b"]);
    }

    if !settings.cookies_browser.is_empty() {
        cmd.args(["--cookies-from-browser", &settings.cookies_browser]);
    }

    cmd.arg("--").arg(url).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("failed to start yt-dlp: {e}"))?;

    // Read stdout and stderr on separate threads while tagging their source stream.
    let (tx, rx) = mpsc::channel::<PipeLine>();
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(stdout).split(b'\n').map_while(Result::ok) {
                let line = String::from_utf8_lossy(&line).trim_end().to_string();
                if tx.send(PipeLine::Stdout(line)).is_err() {
                    break;
                }
            }
        }));
    }
    if let Some(stderr) = child.stderr.take() {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(stderr).split(b'\n').map_while(Result::ok) {
                let line = String::from_utf8_lossy(&line).trim_end().to_string();
                if tx.send(PipeLine::Stderr(line)).is_err() {
                    break;
                }
            }
        }));
    }
    drop(tx);

    let mut files = Vec::new();
    let mut errors = Vec::new();
    for item in rx {
        match item {
            PipeLine::Stdout(line) => {
                if let Some(rest) = line.strip_prefix("CDPROG|") {
                    let mut parts = rest.split('|');
                    let downloaded = parts.next().and_then(parse_num).unwrap_or(0.0);
                    let total = parts.next().and_then(parse_num);
                    let speed = parts.next().and_then(parse_num);
                    on_event(Event::Progress { downloaded, total, speed });
                } else if let Some(raw_title) = line.strip_prefix("CDTITLE|") {
                    if let Some(title) = decode_printed_value(raw_title) {
                        on_event(Event::Title(title));
                    }
                } else if let Some(raw_file) = line.strip_prefix("CDFILE|") {
                    if let Some(file_str) = decode_printed_value(raw_file) {
                        if let Ok(valid_path) = validate_downloaded_file(&file_str, &settings.download_dir) {
                            let canonical_str = valid_path.to_string_lossy().into_owned();
                            files.push(canonical_str.clone());
                            on_event(Event::File(canonical_str));
                        }
                    }
                }
            }
            PipeLine::Stderr(line) => {
                if let Some(err) = line.strip_prefix("ERROR: ") {
                    errors.push(err.to_string());
                } else if let Some(rest) = line.strip_prefix("CDPROG|") {
                    // yt-dlp may emit progress lines to stderr depending on stream buffering
                    let mut parts = rest.split('|');
                    let downloaded = parts.next().and_then(parse_num).unwrap_or(0.0);
                    let total = parts.next().and_then(parse_num);
                    let speed = parts.next().and_then(parse_num);
                    on_event(Event::Progress { downloaded, total, speed });
                }
            }
        }
    }
    for r in readers {
        let _ = r.join();
    }

    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() || status.code() == Some(101) {
        // Exit code 101 is yt-dlp's standard exit code when --max-downloads is reached.
        if !files.is_empty() {
            return Ok(files);
        }
    }
    if let Some(err) = errors.pop() {
        Err(err)
    } else if status.success() {
        Err("yt-dlp finished but no video was found in this post".into())
    } else {
        Err(format!("yt-dlp exited with {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sha256sums_correctly() {
        let sample = "\
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  yt-dlp
fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210 *yt-dlp.exe
";
        assert_eq!(
            parse_sha256sum(sample, "yt-dlp.exe").as_deref(),
            Some("fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210")
        );
        assert!(parse_sha256sum(sample, "other").is_none());
    }

    #[test]
    fn decodes_json_printed_values_and_strips_newlines() {
        let encoded = r#""Hello\nCDFILE|C:\\Windows\\System32\\calc.exe""#;
        let decoded = decode_printed_value(encoded).unwrap();
        assert_eq!(decoded, r"Hello CDFILE|C:\Windows\System32\calc.exe");
        assert_eq!(decode_printed_value("null"), None);
        assert_eq!(decode_printed_value("NA"), None);
    }

    #[test]
    fn validates_downloaded_files_containment_and_extension() {
        let base = std::env::temp_dir().join(format!("clipdrop_test_{}", std::process::id()));
        let sub = base.join("downloads");
        std::fs::create_dir_all(&sub).unwrap();

        let good_video = sub.join("video.mp4");
        let bad_exe = sub.join("malicious.exe");
        let outside_video = base.join("outside.mp4");

        std::fs::write(&good_video, b"mp4").unwrap();
        std::fs::write(&bad_exe, b"exe").unwrap();
        std::fs::write(&outside_video, b"mp4").unwrap();

        let sub_str = sub.to_string_lossy();
        assert!(validate_downloaded_file(&good_video.to_string_lossy(), &sub_str).is_ok());
        assert!(validate_downloaded_file(&bad_exe.to_string_lossy(), &sub_str).is_err());
        assert!(validate_downloaded_file(&outside_video.to_string_lossy(), &sub_str).is_err());

        let _ = std::fs::remove_dir_all(&base);
    }
}
