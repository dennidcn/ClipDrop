//! Thin wrapper around the yt-dlp binary: locating/installing it and running downloads
//! with machine-readable progress output.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::errors::{self, Failure};
use crate::links::Platform;
use crate::settings::Settings;

const EXE_NAME: &str = if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" };
const FFMPEG_EXE_NAME: &str = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };

const MAX_CHECKSUM_BYTES: u64 = 128 * 1024;
const MAX_BINARY_BYTES: u64 = 100 * 1024 * 1024;
const ALLOWED_MEDIA_EXTENSIONS: &[&str] = &["mp4", "webm", "mkv", "mov", "m4a", "mp3"];
/// Largest tool zip accepted (the ffmpeg build is ~200 MB, Deno ~45 MB); leaves headroom for growth.
const MAX_TOOL_ZIP_BYTES: u64 = 400 * 1024 * 1024;
/// Upper bound for each binary extracted from a tool zip.
const MAX_TOOL_EXE_BYTES: u64 = 300 * 1024 * 1024;
const FFMPEG_RELEASE: &str = "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest";
const FFMPEG_WIN64_ASSET: &str = "ffmpeg-master-latest-win64-gpl.zip";
const DENO_EXE_NAME: &str = if cfg!(windows) { "deno.exe" } else { "deno" };
const DENO_RELEASE: &str = "https://github.com/denoland/deno/releases/latest/download";
const DENO_WIN64_ASSET: &str = "deno-x86_64-pc-windows-msvc.zip";

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

/// An ffmpeg already on this machine: the app-managed copy, else one on `PATH` that runs.
pub fn find_ffmpeg(bin_dir: &Path) -> Option<PathBuf> {
    static SYSTEM: OnceLock<Option<PathBuf>> = OnceLock::new();
    find_tool(bin_dir, FFMPEG_EXE_NAME, "-version", &SYSTEM)
}

/// A Deno already on this machine (app-managed copy, else `PATH`). yt-dlp runs YouTube's player
/// JavaScript in it; Deno is yt-dlp's default runtime and runs that code without file or network access.
pub fn find_deno(bin_dir: &Path) -> Option<PathBuf> {
    static SYSTEM: OnceLock<Option<PathBuf>> = OnceLock::new();
    find_tool(bin_dir, DENO_EXE_NAME, "--version", &SYSTEM)
}

fn find_tool(
    bin_dir: &Path,
    exe: &str,
    version_arg: &str,
    system: &'static OnceLock<Option<PathBuf>>,
) -> Option<PathBuf> {
    let managed = bin_dir.join(exe);
    if managed.is_file() {
        return Some(managed);
    }
    system
        .get_or_init(|| {
            let candidate = find_in_path(exe)?;
            runs(&candidate, version_arg).then_some(candidate)
        })
        .clone()
}

fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(120))
        .build()
}

/// Parses a checksum file into the lowercase hex SHA-256 for the named asset.
type ChecksumParser = fn(&str, &str) -> Option<String>;

fn fetch_checksum(agent: &ureq::Agent, sums_url: &str, asset: &str, parse: ChecksumParser) -> Result<String, String> {
    let resp = agent
        .get(sums_url)
        .call()
        .map_err(|e| format!("failed to fetch checksums: {e}"))?;
    let mut text = String::new();
    resp.into_reader()
        .take(MAX_CHECKSUM_BYTES)
        .read_to_string(&mut text)
        .map_err(|e| format!("failed to read checksums: {e}"))?;
    parse(&text, asset).ok_or_else(|| format!("SHA-256 checksum for {asset} not found"))
}

/// Parses PowerShell `Get-FileHash` output (`Hash : <hex>` plus `Path : ...\<asset>`), the
/// format Deno publishes per asset. The `Path` line must name `asset`, binding the hash to it.
fn parse_get_filehash(text: &str, asset: &str) -> Option<String> {
    let field = |key: &str| {
        text.lines().find_map(|line| {
            let (k, v) = line.split_once(':')?;
            (k.trim() == key).then(|| v.trim())
        })
    };
    let file_name = field("Path")?.rsplit(['\\', '/']).next()?;
    if file_name != asset {
        return None;
    }
    let hash = field("Hash")?;
    (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hash.to_ascii_lowercase())
}

/// Streams `url` into `dest`, enforcing `max_bytes`, and returns the file's SHA-256 (lowercase hex).
fn download_hashed(agent: &ureq::Agent, url: &str, dest: &Path, max_bytes: u64) -> Result<String, String> {
    let resp = agent.get(url).call().map_err(|e| format!("download failed: {e}"))?;
    let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut reader = resp.into_reader().take(max_bytes + 1);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("download failed: {e}"))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > max_bytes {
            drop(file);
            let _ = std::fs::remove_file(dest);
            return Err("download exceeded the maximum allowed size".into());
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| format!("write failed: {e}"))?;
    }
    file.flush().map_err(|e| e.to_string())?;
    Ok(hex_lower(&hasher.finalize()))
}

/// Downloads the yt-dlp project's Windows ffmpeg build, verifies its SHA-256, and extracts
/// `ffmpeg` and `ffprobe` into `bin_dir`. Other platforms must provide ffmpeg on `PATH`.
pub fn install_ffmpeg(bin_dir: &Path) -> Result<PathBuf, String> {
    install_zip_tool(&ZipTool {
        name: "ffmpeg",
        sums_url: &format!("{FFMPEG_RELEASE}/checksums.sha256"),
        parse_checksum: parse_sha256sum,
        zip_url: &format!("{FFMPEG_RELEASE}/{FFMPEG_WIN64_ASSET}"),
        asset: FFMPEG_WIN64_ASSET,
        // ffprobe first: the main exe's presence is what marks the install as complete.
        entries: &["bin/ffprobe.exe", "bin/ffmpeg.exe"],
        version_arg: "-version",
        bin_dir,
    })
}

/// Downloads Deno's official Windows build, verifies its SHA-256, and extracts `deno.exe` into `bin_dir`.
pub fn install_deno(bin_dir: &Path) -> Result<PathBuf, String> {
    install_zip_tool(&ZipTool {
        name: "Deno",
        sums_url: &format!("{DENO_RELEASE}/{DENO_WIN64_ASSET}.sha256sum"),
        parse_checksum: parse_get_filehash,
        zip_url: &format!("{DENO_RELEASE}/{DENO_WIN64_ASSET}"),
        asset: DENO_WIN64_ASSET,
        entries: &["deno.exe"],
        version_arg: "--version",
        bin_dir,
    })
}

/// A command-line tool shipped as a zip with a published SHA-256.
struct ZipTool<'a> {
    name: &'a str,
    sums_url: &'a str,
    parse_checksum: ChecksumParser,
    zip_url: &'a str,
    asset: &'a str,
    /// Archive paths (matched as a whole name or a `/`-separated suffix) of the exes to extract.
    /// The last one is the main exe, returned and checked with `version_arg`.
    entries: &'a [&'a str],
    version_arg: &'a str,
    bin_dir: &'a Path,
}

/// Downloads a tool's zip, verifies it against the published SHA-256, and extracts its exes into
/// `bin_dir`. Only Windows x64 builds are fetched; elsewhere the tool must be on `PATH`.
fn install_zip_tool(tool: &ZipTool) -> Result<PathBuf, String> {
    let name = tool.name;
    if !(cfg!(windows) && cfg!(target_arch = "x86_64")) {
        return Err(format!("install {name} and make sure it is on your PATH"));
    }
    let bin_dir = tool.bin_dir;
    std::fs::create_dir_all(bin_dir).map_err(|e| format!("cannot create {}: {e}", bin_dir.display()))?;
    let agent = http_agent();
    let expected = fetch_checksum(&agent, tool.sums_url, tool.asset, tool.parse_checksum)?;

    let zip_path = bin_dir.join(format!("{}.part", tool.asset));
    let actual = download_hashed(&agent, tool.zip_url, &zip_path, MAX_TOOL_ZIP_BYTES)?;
    if actual != expected {
        let _ = std::fs::remove_file(&zip_path);
        return Err(format!("{name} SHA-256 verification failed (expected {expected}, got {actual})"));
    }
    let result = extract_zip_tool(tool, &zip_path);
    let _ = std::fs::remove_file(&zip_path);
    result
}

fn extract_zip_tool(tool: &ZipTool, zip_path: &Path) -> Result<PathBuf, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("bad {} archive: {e}", tool.name))?;
    let mut main_exe = None;
    for entry_path in tool.entries {
        // Entries are matched by name and written to fixed file names, so archive paths can't escape bin_dir.
        let exe = entry_path.rsplit('/').next().unwrap_or(entry_path);
        let suffix = format!("/{entry_path}");
        let name = archive
            .file_names()
            .filter_map(Result::ok)
            .find(|n| n == entry_path || n.ends_with(&suffix))
            .map(|n| n.into_owned())
            .ok_or_else(|| format!("{exe} missing from {} archive", tool.name))?;
        let mut entry = archive.by_name(&name).map_err(|e| e.to_string())?;
        let tmp = tool.bin_dir.join(format!("{exe}.part"));
        let mut out = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let copied = std::io::copy(&mut (&mut entry).take(MAX_TOOL_EXE_BYTES + 1), &mut out)
            .map_err(|e| format!("failed to extract {exe}: {e}"))?;
        drop(out);
        if copied > MAX_TOOL_EXE_BYTES {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("{exe} in archive is unexpectedly large"));
        }
        let dest = tool.bin_dir.join(exe);
        std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
        main_exe = Some(dest);
    }
    let main_exe = main_exe.ok_or("no files to extract")?;
    if !runs(&main_exe, tool.version_arg) {
        let _ = std::fs::remove_file(&main_exe);
        return Err(format!("the downloaded {} does not run", tool.name));
    }
    Ok(main_exe)
}

fn parse_sha256sum(sums_text: &str, asset_name: &str) -> Option<String> {
    for line in sums_text.lines() {
        let mut parts = line.split_whitespace();
        // Skip blank or malformed lines rather than giving up on the whole file.
        let (Some(hash), Some(name)) = (parts.next(), parts.next()) else { continue };
        let name = name.trim_start_matches('*');
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

    let agent = http_agent();
    let asset = release_asset();
    let release = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
    let expected_sha256 =
        fetch_checksum(&agent, &format!("{release}/SHA2-256SUMS"), asset, parse_sha256sum).map_err(|e| format!("yt-dlp: {e}"))?;

    let tmp = bin_dir.join(format!("{EXE_NAME}.part"));
    let actual_sha256 = download_hashed(&agent, &format!("{release}/{asset}"), &tmp, MAX_BINARY_BYTES)
        .map_err(|e| format!("yt-dlp: {e}"))?;

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
    /// The download finished and ffmpeg is converting it to MP3.
    Converting,
    /// Where yt-dlp is about to save a file; its partial files are named after it.
    Destination(String),
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
    // dunce avoids Windows verbatim (`\\?\C:\...`) paths, which shell APIs (open/reveal) often reject.
    let canon_dir = dunce::canonicalize(download_dir)
        .map_err(|_| "Download folder does not exist.".to_string())?;
    let canon_file = dunce::canonicalize(file)
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

/// What to save from a link: chosen by which box the link was pasted into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// The video, as an mp4.
    Mp4,
    /// The audio, converted to mp3 with tags and cover art.
    Mp3,
}

impl Format {
    /// Whether this download can't be done properly without ffmpeg (so it is installed if missing).
    /// MP3 always converts; YouTube serves good video only as separate streams that ffmpeg merges;
    /// and yt-dlp cuts clips with ffmpeg.
    pub fn needs_ffmpeg(self, platform: Platform, clip: Option<Clip>) -> bool {
        self == Format::Mp3 || platform == Platform::Youtube || clip.is_some()
    }
}

/// Part of a video to save (yt-dlp's `--download-sections`), in whole seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Clip {
    pub start: u32,
    /// `None`: to the end.
    pub end: Option<u32>,
}

/// Longest time accepted in a clip box (100 hours), so nothing overflows.
const MAX_CLIP_SECONDS: u32 = 100 * 3600;

impl Clip {
    /// Reads the From and To boxes. Both empty means the whole video. Times are `1:10`,
    /// `1:02:03` or plain seconds; an empty From starts at the beginning, an empty To runs to the end.
    pub fn parse(start: &str, end: &str) -> Result<Option<Clip>, String> {
        let (start, end) = (start.trim(), end.trim());
        if start.is_empty() && end.is_empty() {
            return Ok(None);
        }
        let time = |s: &str| {
            parse_time(s).ok_or_else(|| format!("\"{s}\" isn't a time. Use minutes and seconds, like 1:10."))
        };
        let start = if start.is_empty() { 0 } else { time(start)? };
        let end = if end.is_empty() { None } else { Some(time(end)?) };
        if end.is_some_and(|end| end <= start) {
            return Err("The end of the clip has to come after its start.".into());
        }
        if start == 0 && end.is_none() {
            return Ok(None);
        }
        Ok(Some(Clip { start, end }))
    }

    /// yt-dlp's `--download-sections` value, e.g. `*30-70` or `*30-inf`.
    fn section(self) -> String {
        match self.end {
            Some(end) => format!("*{}-{end}", self.start),
            None => format!("*{}-inf", self.start),
        }
    }

    /// Added to the file name so a clip doesn't collide with the whole video or other clips:
    /// ` (clip 0m30s-1m10s)`. Windows file names can't contain `:`.
    fn file_suffix(self) -> String {
        let t = |s: u32| {
            if s >= 3600 {
                format!("{}h{:02}m{:02}s", s / 3600, s / 60 % 60, s % 60)
            } else {
                format!("{}m{:02}s", s / 60, s % 60)
            }
        };
        let end = self.end.map_or_else(|| "end".to_string(), t);
        format!(" (clip {}-{end})", t(self.start))
    }
}

/// `1:10` -> 70, `1:02:03` -> 3723, `45` -> 45.
fn parse_time(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty() || p.len() > 6 || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let mut total: u64 = 0;
    for (i, part) in parts.iter().enumerate() {
        let n: u64 = part.parse().ok()?;
        // Minutes and seconds after the first part must be under 60.
        if i > 0 && n >= 60 {
            return None;
        }
        total = total * 60 + n;
    }
    u32::try_from(total).ok().filter(|t| *t <= MAX_CLIP_SECONDS)
}

/// What a finished download left in the folder.
#[derive(Debug)]
pub struct Saved {
    pub files: Vec<String>,
    /// The file was already in the folder from an earlier download, so nothing was downloaded.
    pub already_saved: bool,
}

/// Whether yt-dlp needs a JavaScript runtime (Deno) for this site.
pub fn needs_js_runtime(platform: Platform) -> bool {
    platform == Platform::Youtube
}

enum PipeLine {
    Stdout(String),
    Stderr(String),
}

/// Downloads `url` into the configured directory (just `clip` of it, if set).
#[allow(clippy::too_many_arguments)]
pub fn download(
    ytdlp: &Path,
    url: &str,
    platform: Platform,
    format: Format,
    clip: Option<Clip>,
    settings: &Settings,
    ffmpeg: Option<&Path>,
    deno: Option<&Path>,
    stopper: &Stopper,
    mut on_event: impl FnMut(Event),
) -> Result<Saved, Failure> {
    let mut settings = settings.clone();
    settings.validate()?;
    if format.needs_ffmpeg(platform, clip) && ffmpeg.is_none() {
        return Err(Failure::from(match format {
            Format::Mp3 => "Converting to MP3 needs ffmpeg, which isn't available.".to_string(),
            Format::Mp4 if clip.is_some() => "Saving part of a video needs ffmpeg, which isn't available.".into(),
            Format::Mp4 => format!("{} videos need ffmpeg, which isn't available.", platform.label()),
        }));
    }

    std::fs::create_dir_all(&settings.download_dir)
        .map_err(|e| format!("cannot create download folder: {e}"))?;

    let mut cmd = command(ytdlp);
    cmd.args(build_args(platform, format, clip, &settings, ffmpeg, deno));
    cmd.arg("--").arg(url).stdout(Stdio::piped()).stderr(Stdio::piped());
    if stopper.is_stopped() {
        return Err(STOPPED.to_string().into());
    }
    let mut child = cmd.spawn().map_err(|e| format!("failed to start yt-dlp: {e}"))?;
    stopper.attach(contain(&child));
    // The match filter skips live streams silently (as do some other limits), so hint at the likely cause.
    let nothing_found = match format {
        Format::Mp3 => "Nothing was saved. If this is a live stream, try again once it has ended.",
        Format::Mp4 => "No video was found in this post. If it's a live stream, try again once it has ended.",
    };
    let output = Output { download_dir: &settings.download_dir, format, nothing_found, stopper };
    let result = read_output(&mut child, &output, &mut on_event).map_err(|raw| match raw {
        RawError::YtDlp(e) => errors::explain(&e, platform, &settings.cookies_browser),
        RawError::Own(e) => Failure::from(e),
    });
    // Closing the download's job handle also ends anything yt-dlp left behind.
    stopper.detach();
    if stopper.is_stopped() && result.is_err() {
        return Err(STOPPED.to_string().into());
    }
    result
}

const STOPPED: &str = "The download was stopped.";

/// Lets another thread end a running download (the pause and stop buttons). A stop that comes
/// in before yt-dlp has started still takes effect: the download then doesn't start at all.
#[derive(Default)]
pub struct Stopper(Mutex<StopState>);

#[derive(Default)]
struct StopState {
    stopped: bool,
    killer: Option<Killer>,
}

impl Stopper {
    fn lock(&self) -> std::sync::MutexGuard<'_, StopState> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Ends the download's yt-dlp process and everything it started.
    pub fn stop(&self) {
        let mut state = self.lock();
        state.stopped = true;
        if let Some(killer) = &state.killer {
            killer.kill();
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.lock().stopped
    }

    fn attach(&self, killer: Killer) {
        let mut state = self.lock();
        if state.stopped {
            killer.kill();
        }
        state.killer = Some(killer);
    }

    fn detach(&self) {
        self.lock().killer = None;
    }
}

/// Ends one download's process tree.
enum Killer {
    /// The download's own job object (nested in the app-wide one).
    #[cfg(windows)]
    Job(job::Owned),
    /// Fallback when no job object could be made: kill by process id.
    Pid(u32),
}

impl Killer {
    fn kill(&self) {
        match self {
            #[cfg(windows)]
            Killer::Job(job) => job.terminate(),
            Killer::Pid(pid) => kill_tree(*pid),
        }
    }
}

fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let Some(root) = std::env::var_os("SystemRoot") else { return };
        let taskkill = PathBuf::from(root).join("System32").join("taskkill.exe");
        let _ = command(taskkill).args(["/T", "/F", "/PID", &pid.to_string()]).status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).status();
    }
}

/// Deletes what a stopped download left in the folder: `.part`/`.ytdl` files, unmerged streams
/// (`name.f137.mp4`), ffmpeg temp files and thumbnails. `destinations` are the paths yt-dlp
/// reported (see `Event::Destination`). Files that look finished (`name.mp4`) are kept.
pub fn remove_partial_files(destinations: &[String], download_dir: &str) {
    let Ok(dir) = dunce::canonicalize(download_dir) else { return };
    let mut doomed = Vec::new();
    for dest in destinations {
        let dest = Path::new(dest);
        let (Some(parent), Some(stem)) = (dest.parent(), dest.file_stem().and_then(|s| s.to_str())) else {
            continue;
        };
        let Ok(parent) = dunce::canonicalize(parent) else { continue };
        if !parent.starts_with(&dir) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&parent) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(stem)?.strip_prefix('.')) else {
                continue;
            };
            let finished = ALLOWED_MEDIA_EXTENSIONS.iter().any(|ext| rest.eq_ignore_ascii_case(ext));
            if !finished && entry.file_type().is_ok_and(|t| t.is_file()) {
                doomed.push(entry.path());
            }
        }
    }
    // A just-killed process can hold its files open for a moment, so retry briefly.
    for _ in 0..20 {
        doomed.retain(|path| std::fs::remove_file(path).is_err() && path.exists());
        if doomed.is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    for path in doomed {
        eprintln!("[clipdrop] couldn't delete partial file {}", path.display());
    }
}

/// Puts a yt-dlp process in a Windows job object that kills it, and everything it starts (ffmpeg,
/// Deno), as soon as ClipDrop exits for any reason (quit, update install, crash), so nothing keeps
/// running hidden. Assigned right after spawn: yt-dlp's launcher unpacks itself before starting
/// children, and children started afterwards inherit the job.
///
/// Each download also gets its own job, nested inside the app-wide one, so the pause and stop
/// buttons can end just that download's process tree. Returns what ends it.
#[cfg(windows)]
fn contain(child: &std::process::Child) -> Killer {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
    let process = child.as_raw_handle();
    if let Some(job) = job::handle() {
        // SAFETY: both handles are valid; the child's handle is owned by `child`, which outlives the call.
        if unsafe { AssignProcessToJobObject(job, process) } == 0 {
            eprintln!("[clipdrop] couldn't add yt-dlp to the job object: {}", std::io::Error::last_os_error());
        }
    }
    if let Some(own) = job::Owned::new() {
        // SAFETY: as above; `own` holds a valid job handle.
        if unsafe { AssignProcessToJobObject(own.raw(), process) } != 0 {
            return Killer::Job(own);
        }
        eprintln!("[clipdrop] couldn't add yt-dlp to its own job: {}", std::io::Error::last_os_error());
    }
    Killer::Pid(child.id())
}

#[cfg(not(windows))]
fn contain(child: &std::process::Child) -> Killer {
    Killer::Pid(child.id())
}

/// Stops every running download now (yt-dlp and its children). Called just before the app exits,
/// e.g. so an update installer doesn't find ffmpeg still holding files open.
pub fn kill_all() {
    #[cfg(windows)]
    if let Some(job) = job::handle() {
        // SAFETY: `job` is a valid job object handle for the life of the process.
        unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(job, 1) };
    }
}

#[cfg(windows)]
mod job {
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    };

    /// The process-wide job. Its handle is never closed, so Windows closes it when ClipDrop exits,
    /// which (with KILL_ON_JOB_CLOSE) terminates every process still in the job.
    pub fn handle() -> Option<HANDLE> {
        static JOB: OnceLock<Option<usize>> = OnceLock::new();
        JOB.get_or_init(create).map(|h| h as HANDLE)
    }

    /// A job owned by one download. Dropping it closes the handle, which (with KILL_ON_JOB_CLOSE)
    /// ends anything still running in it.
    pub struct Owned(usize);

    impl Owned {
        pub fn new() -> Option<Self> {
            create().map(Owned)
        }

        pub fn raw(&self) -> HANDLE {
            self.0 as HANDLE
        }

        pub fn terminate(&self) {
            // SAFETY: the handle stays valid until `self` is dropped.
            unsafe { TerminateJobObject(self.raw(), 1) };
        }
    }

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: the handle was created by `create` and is closed only here.
            unsafe { CloseHandle(self.raw()) };
        }
    }

    fn create() -> Option<usize> {
        // SAFETY: plain Win32 calls with valid pointers; the info struct is zero-initialised as documented.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                eprintln!("[clipdrop] couldn't create job object: {}", std::io::Error::last_os_error());
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                eprintln!("[clipdrop] couldn't configure job object: {}", std::io::Error::last_os_error());
                return None;
            }
            Some(job as usize)
        }
    }
}

/// The yt-dlp options (everything before `-- URL`) used for a download.
fn build_args(
    platform: Platform,
    format: Format,
    clip: Option<Clip>,
    settings: &Settings,
    ffmpeg: Option<&Path>,
    deno: Option<&Path>,
) -> Vec<std::ffi::OsString> {
    let mut cmd = ArgList::default();
    cmd.args([
        // Never read yt-dlp config files: one dropped into the download folder (e.g. a drive-by
        // download of yt-dlp.conf) would be applied, and could add --exec or lift the extractor allowlist.
        "--ignore-config",
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
        "--use-extractors",
        platform.allowed_extractors(),
        // A live stream has no end, so it would record for hours (and has no audio-only format).
        "--match-filter",
        "!is_live",
    ])
    .args(["-P", &format!("home:{}", settings.download_dir)]);
    // Both formats are named after the post's title, so files are easy to find in Explorer. The two
    // names must differ by more than the extension: an MP3 job first downloads e.g. `name.mp4`, and
    // if a video by that name were already there, yt-dlp would treat it as its own download,
    // convert it and then delete it. A different name also keeps the stop button's partial-file
    // cleanup (which matches on the name) from touching the other format's files.
    cmd.arg("-o").arg(output_template(format, clip));
    if let Some(clip) = clip {
        cmd.args(["--download-sections", &clip.section()]);
    }
    cmd
    // Use JSON formatting (%()j) so embedded newlines in post titles are escaped and cannot spoof CDFILE| lines.
    .args(["--print", "before_dl:CDTITLE|%(title.0:120)j"])
    .args(["--print", "before_dl:CDDEST|%(filename)j"])
    .args(["--print", "after_move:CDFILE|%(filepath)j"])
    .args([
        "--progress-template",
        "download:CDPROG|%(progress.downloaded_bytes)s|%(progress.total_bytes,progress.total_bytes_estimate)s|%(progress.speed)s",
        "--progress-template",
        "postprocess:CDPP|%(progress.status)s|%(progress.postprocessor)s",
    ]);

    if let Some(deno) = deno.filter(|_| needs_js_runtime(platform)) {
        let mut runtime = std::ffi::OsString::from("deno:");
        runtime.push(deno);
        cmd.arg("--js-runtimes").arg(runtime);
    }

    if format == Format::Mp3 {
        if let Some(ffmpeg) = ffmpeg {
            cmd.arg("--ffmpeg-location").arg(ffmpeg);
        }
        cmd.args([
            "-f",
            "ba/b",
            "--extract-audio",
            "--audio-format",
            "mp3",
            "--audio-quality",
            "0",
            "--embed-metadata",
            "--embed-thumbnail",
            "--convert-thumbnails",
            "jpg",
        ]);
    } else if let Some(ffmpeg) = ffmpeg {
        cmd.arg("--ffmpeg-location").arg(ffmpeg).args([
            "-f",
            &video_format(settings.video_quality.max_height()),
            "--merge-output-format",
            "mp4",
        ]);
    } else {
        // Without ffmpeg we can't merge separate audio/video streams, so stick to single-file formats.
        let format = match settings.video_quality.max_height() {
            Some(h) => format!("b[height<={h}][ext=mp4]/b[height<={h}]/b[ext=mp4]/b"),
            None => "b[ext=mp4]/b".into(),
        };
        cmd.args(["-f", &format]);
    }

    if !settings.cookies_browser.is_empty() {
        cmd.args(["--cookies-from-browser", &settings.cookies_browser]);
    }
    cmd.0
}

/// yt-dlp's `-f` for video with ffmpeg available. Capped (720p or 1080p): prefer H.264, which plays
/// everywhere, unlike AV1/VP9, and keeps YouTube videos well under --max-filesize. Uncapped ("Best"):
/// the highest resolution offered, which on YouTube means VP9 or AV1 for 1440p and 4K.
fn video_format(max_height: Option<u32>) -> String {
    match max_height {
        Some(h) => format!(
            "bv*[vcodec^=avc1][height<={h}]+ba[ext=m4a]/bv*[height<={h}][ext=mp4]+ba[ext=m4a]/bv*[height<={h}]+ba/b[height<={h}]/b"
        ),
        None => "bv*+ba/b".into(),
    }
}

/// yt-dlp's `-o` file name for each format (see `build_args` for why they differ). A clip gets
/// its range in the name, so it doesn't clash with the whole video or another clip of it.
fn output_template(format: Format, clip: Option<Clip>) -> String {
    let clip = clip.map(Clip::file_suffix).unwrap_or_default();
    match format {
        Format::Mp3 => format!("%(title).90s{clip} [%(id)s].%(ext)s"),
        Format::Mp4 => format!("%(title).90s{clip} (%(id)s).%(ext)s"),
    }
}

/// Collects command-line arguments with the same `arg`/`args` shape as `Command`.
#[derive(Default)]
struct ArgList(Vec<std::ffi::OsString>);

impl ArgList {
    fn arg(&mut self, a: impl AsRef<std::ffi::OsStr>) -> &mut Self {
        self.0.push(a.as_ref().to_os_string());
        self
    }
    fn args<I: IntoIterator<Item = S>, S: AsRef<std::ffi::OsStr>>(&mut self, items: I) -> &mut Self {
        for a in items {
            self.arg(a);
        }
        self
    }
}

/// Handles a download (`CDPROG|`) or post-processing (`CDPP|`) progress line. Returns whether it was one.
fn progress_event(line: &str, on_event: &mut impl FnMut(Event)) -> bool {
    if let Some(rest) = line.strip_prefix("CDPROG|") {
        let mut parts = rest.split('|');
        let downloaded = parts.next().and_then(parse_num).unwrap_or(0.0);
        let total = parts.next().and_then(parse_num);
        let speed = parts.next().and_then(parse_num);
        on_event(Event::Progress { downloaded, total, speed });
        true
    } else if let Some(rest) = line.strip_prefix("CDPP|") {
        if rest == "started|ExtractAudio" {
            on_event(Event::Converting);
        }
        true
    } else {
        false
    }
}

fn read_output(
    child: &mut std::process::Child,
    out: &Output,
    on_event: &mut impl FnMut(Event),
) -> Result<Saved, RawError> {
    let download_dir = out.download_dir;
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
    let mut started = false;
    let mut already_saved = false;
    for item in rx {
        match item {
            PipeLine::Stdout(line) => {
                if progress_event(&line, on_event) {
                    started = true;
                } else if let Some(raw_title) = line.strip_prefix("CDTITLE|") {
                    if let Some(title) = decode_printed_value(raw_title) {
                        on_event(Event::Title(title));
                    }
                } else if let Some(raw_dest) = line.strip_prefix("CDDEST|") {
                    if let Some(dest) = decode_printed_value(raw_dest) {
                        // Saved by an earlier download (maybe before a restart)? Then stop here,
                        // before anything is fetched, and point at that file. Only for the first
                        // file: in a post with several videos, yt-dlp skips the ones it already has.
                        let existing = (!started && files.is_empty())
                            .then(|| finished_file(&dest, out.format, download_dir))
                            .flatten();
                        on_event(Event::Destination(dest));
                        if let Some(existing) = existing {
                            out.stopper.stop();
                            already_saved = true;
                            on_event(Event::File(existing.clone()));
                            files.push(existing);
                        }
                    }
                } else if let Some(raw_file) = line.strip_prefix("CDFILE|") {
                    if let Some(file_str) = decode_printed_value(raw_file) {
                        match validate_downloaded_file(&file_str, download_dir) {
                            Ok(valid_path) => {
                                let canonical_str = valid_path.to_string_lossy().into_owned();
                                files.push(canonical_str.clone());
                                on_event(Event::File(canonical_str));
                            }
                            // Surface why a finished download was discarded instead of "no video found".
                            Err(e) => errors.push(RawError::Own(e)),
                        }
                    }
                }
            }
            PipeLine::Stderr(line) => {
                if let Some(err) = line.strip_prefix("ERROR: ") {
                    errors.push(RawError::YtDlp(err.to_string()));
                } else {
                    // In quiet mode (implied by --print) yt-dlp writes progress to stderr.
                    started |= progress_event(&line, on_event);
                }
            }
        }
    }
    for r in readers {
        let _ = r.join();
    }

    let status = child.wait().map_err(|e| RawError::Own(e.to_string()))?;
    if already_saved {
        // yt-dlp was ended on purpose, so its exit status doesn't matter.
        return Ok(Saved { files, already_saved });
    }
    if status.success() || status.code() == Some(101) {
        // Exit code 101 is yt-dlp's standard exit code when --max-downloads is reached.
        if !files.is_empty() {
            return Ok(Saved { files, already_saved });
        }
    }
    if let Some(err) = errors.pop() {
        Err(err)
    } else if status.success() {
        Err(RawError::Own(out.nothing_found.into()))
    } else {
        Err(RawError::Own(format!("yt-dlp exited with {status}")))
    }
}

/// What `read_output` needs besides the process.
struct Output<'a> {
    download_dir: &'a str,
    format: Format,
    nothing_found: &'a str,
    /// Used to end yt-dlp when the file turns out to be saved already.
    stopper: &'a Stopper,
}

/// A failed download's message: yt-dlp's own (explained for the user later) or one of ours.
#[derive(Debug, PartialEq)]
enum RawError {
    YtDlp(String),
    Own(String),
}

/// The finished file a download headed for `dest` (yt-dlp's planned file name) would leave,
/// if it is already in the download folder. An MP3 job's planned name has the source's extension
/// (`.webm`, `.m4a`) until it is converted; a video's is already the final `.mp4`.
fn finished_file(dest: &str, format: Format, download_dir: &str) -> Option<String> {
    let dest = Path::new(dest);
    let finished = match format {
        Format::Mp3 => dest.with_extension("mp3"),
        Format::Mp4 => dest.to_path_buf(),
    };
    let valid = validate_downloaded_file(&finished.to_string_lossy(), download_dir).ok()?;
    Some(valid.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::VideoQuality;

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
        // A blank or malformed line earlier in the file doesn't stop the search.
        let messy = format!("\n\nnot-a-checksum-line\n{sample}");
        assert!(parse_sha256sum(&messy, "yt-dlp.exe").is_some());
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

    /// A real yt-dlp to check our arguments against: `CLIPDROP_YTDLP`, the app-managed copy, or `PATH`.
    /// `kill_all` ends every contained process, so tests that use the app-wide job take turns.
    #[cfg(windows)]
    static JOB_TESTS: Mutex<()> = Mutex::new(());

    fn real_ytdlp() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("CLIPDROP_YTDLP").map(PathBuf::from) {
            return Some(p);
        }
        let managed = std::env::var_os("LOCALAPPDATA")
            .map(|d| PathBuf::from(d).join("com.clipdrop.app").join("bin").join(EXE_NAME))
            .filter(|p| p.is_file());
        managed.or_else(|| find_in_path(EXE_NAME))
    }

    fn test_settings() -> Settings {
        Settings {
            download_dir: std::env::temp_dir().to_string_lossy().into_owned(),
            cookies_browser: "firefox".into(),
            ..Default::default()
        }
    }

    /// Runs yt-dlp with our real download options, pointed at a dead proxy so nothing leaves the machine.
    fn run_real(ytdlp: &Path, platform: Platform, url: &str) -> String {
        let mut settings = test_settings();
        settings.cookies_browser.clear(); // don't touch the developer's browser profile
        let out = command(ytdlp)
            .args(build_args(platform, Format::Mp4, None, &settings, None, None))
            .args(["--simulate", "--ignore-config", "--proxy", "http://127.0.0.1:9", "--retries", "0"])
            .arg("--")
            .arg(url)
            .output()
            .expect("run yt-dlp");
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
    }

    #[test]
    fn real_ytdlp_accepts_our_options_and_extractors() {
        let Some(ytdlp) = real_ytdlp() else {
            eprintln!("skipping: no yt-dlp found (set CLIPDROP_YTDLP)");
            return;
        };

        // Option parsing happens before `--version` prints, so unknown flags fail here (offline).
        for platform in [Platform::Twitter, Platform::Instagram, Platform::Tiktok, Platform::Youtube] {
            for format in [Format::Mp4, Format::Mp3] {
                for ffmpeg in [Some(ytdlp.as_path()), None] {
                    let out = command(&ytdlp)
                        .args(build_args(platform, format, None, &test_settings(), ffmpeg, Some(&ytdlp)))
                        .arg("--version")
                        .output()
                        .expect("run yt-dlp");
                    assert!(
                        out.status.success(),
                        "yt-dlp rejected options for {platform:?} {format:?}: {}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                }
            }
        }

        for (platform, url) in [
            (Platform::Twitter, "https://x.com/user/status/1"),
            (Platform::Instagram, "https://www.instagram.com/reel/abc/"),
            (Platform::Tiktok, "https://www.tiktok.com/@user/video/123"),
            (Platform::Tiktok, "https://vm.tiktok.com/ZMabc/"),
            (Platform::Tiktok, "https://www.tiktok.com/t/ZTabc/"),
            (Platform::Youtube, "https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
        ] {
            let out = run_real(&ytdlp, platform, url);
            assert!(!out.contains("No suitable extractor"), "{url} not matched by allowlist:\n{out}");
        }

        // The allowlist must block pivoting to other sites.
        let out = run_real(&ytdlp, Platform::Twitter, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        assert!(out.contains("No suitable extractor"), "allowlist let a YouTube URL through:\n{out}");
        let out = run_real(&ytdlp, Platform::Youtube, "https://x.com/user/status/1");
        assert!(out.contains("No suitable extractor"), "YouTube allowlist let an X URL through:\n{out}");
    }

    #[test]
    fn progress_lines_drive_events() {
        let mut events = Vec::new();
        let mut on_event = |e: Event| {
            events.push(match e {
                Event::Progress { downloaded, .. } => format!("progress {downloaded}"),
                Event::Converting => "converting".into(),
                Event::Title(_) | Event::Destination(_) | Event::File(_) => "other".into(),
            })
        };
        assert!(progress_event("CDPROG|512|1024|10", &mut on_event));
        assert!(progress_event("CDPP|started|ExtractAudio", &mut on_event));
        // Other post-processor lines are consumed without an event.
        assert!(progress_event("CDPP|finished|ExtractAudio", &mut on_event));
        assert!(progress_event("CDPP|started|MoveFiles", &mut on_event));
        assert!(!progress_event("[download] 50%", &mut on_event));
        assert_eq!(events, ["progress 512", "converting"]);
    }

    #[test]
    fn format_decides_the_yt_dlp_options() {
        let tool = Path::new(r"C:\tools\x.exe");
        let args_for = |platform, format, ffmpeg| -> Vec<String> {
            build_args(platform, format, None, &test_settings(), ffmpeg, Some(tool))
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };

        // MP3 from YouTube: audio extraction, title naming, Deno, live streams skipped.
        let args = args_for(Platform::Youtube, Format::Mp3, Some(tool));
        let has = |pair: [&str; 2]| args.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1]);
        assert_eq!(args[0], "--ignore-config");
        assert!(has(["--use-extractors", "youtube"]));
        assert!(has(["--audio-format", "mp3"]));
        assert!(has(["--match-filter", "!is_live"]));
        assert!(has(["--ffmpeg-location", r"C:\tools\x.exe"]));
        assert!(has(["--js-runtimes", r"deno:C:\tools\x.exe"]));
        assert!(has(["-o", "%(title).90s [%(id)s].%(ext)s"]));

        // MP3 from TikTok: audio extraction, but no JS runtime (only YouTube needs it).
        let tiktok = args_for(Platform::Tiktok, Format::Mp3, Some(tool));
        assert!(tiktok.iter().any(|a| a == "--extract-audio"));
        assert!(!tiktok.iter().any(|a| a == "--js-runtimes"));

        // MP4 from YouTube: merged video with Deno, no audio extraction.
        let yt_video = args_for(Platform::Youtube, Format::Mp4, Some(tool));
        assert!(yt_video.iter().any(|a| a == "--js-runtimes"));
        assert!(yt_video.windows(2).any(|w| w[0] == "--merge-output-format" && w[1] == "mp4"));
        assert!(!yt_video.iter().any(|a| a == "--extract-audio"));
        assert!(yt_video.windows(2).any(|w| w[0] == "-o" && w[1] == "%(title).90s (%(id)s).%(ext)s"));
        // Same post as MP3 and MP4: the names must differ before the extension (see build_args).
        let stem = |t: &str| t.trim_end_matches(".%(ext)s").to_string();
        assert_ne!(stem(&output_template(Format::Mp3, None)), stem(&output_template(Format::Mp4, None)));

        // MP4 from X without ffmpeg: single-file formats; every job keeps the safety options.
        let video = args_for(Platform::Twitter, Format::Mp4, None);
        assert_eq!(video[0], "--ignore-config");
        assert!(video.windows(2).any(|w| w[0] == "-f" && w[1] == "b[height<=1080][ext=mp4]/b[height<=1080]/b[ext=mp4]/b"));
        assert!(video.windows(2).any(|w| w[0] == "--match-filter" && w[1] == "!is_live"));
        assert!(!video.iter().any(|a| a == "--extract-audio" || a == "--js-runtimes"));
    }

    /// The job object must take down a contained process *and* the processes it starts.
    #[cfg(windows)]
    #[test]
    fn kill_all_stops_contained_process_tree() {
        let _serial = JOB_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let system32 = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        let ping = system32.join("PING.EXE").to_string_lossy().into_owned();
        // cmd.exe starts a ping child, like yt-dlp starting ffmpeg.
        let mut child = command(system32.join("cmd.exe"))
            .args(["/C", &ping, "-n", "60", "127.0.0.1"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        contain(&child);
        std::thread::sleep(Duration::from_millis(500));
        kill_all();
        let started = std::time::Instant::now();
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(5), "contained process survived kill_all");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn parses_get_filehash_checksums() {
        let deno = "\r\nAlgorithm : SHA256\r\nHash      : CBA84B26E0DF5AE7F74FC66B86A95B866872A86292EBD84FA1E959927EDD93BF\r\nPath      : C:\\a\\deno.zip\r\n";
        assert_eq!(
            parse_get_filehash(deno, "deno.zip").as_deref(),
            Some("cba84b26e0df5ae7f74fc66b86a95b866872a86292ebd84fa1e959927edd93bf")
        );
        // The hash must belong to the asset named in the file.
        assert!(parse_get_filehash(deno, "other.zip").is_none());
        assert!(parse_get_filehash("Hash : not-a-hash\nPath : C:\\deno.zip", "deno.zip").is_none());
        assert!(parse_get_filehash("Path : C:\\deno.zip", "deno.zip").is_none());
    }

    /// Network test: installs ffmpeg and Deno into `CLIPDROP_TEST_DIR` and saves a 19 s YouTube video
    /// as MP3 and MP4, then again (found as already saved), then a clip of it.
    /// Run with `CLIPDROP_TEST_DIR=<dir> cargo test youtube_to_mp3 -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn youtube_to_mp3_end_to_end() {
        let ytdlp = real_ytdlp().expect("no yt-dlp found (set CLIPDROP_YTDLP)");
        let base = PathBuf::from(std::env::var_os("CLIPDROP_TEST_DIR").expect("set CLIPDROP_TEST_DIR"));
        let bin_dir = base.join("bin");
        let ffmpeg = find_ffmpeg(&bin_dir).map_or_else(|| install_ffmpeg(&bin_dir), Ok).expect("ffmpeg");
        let deno = find_deno(&bin_dir).map_or_else(|| install_deno(&bin_dir), Ok).expect("deno");
        let out = base.join("out");
        let _ = std::fs::remove_dir_all(&out);
        let settings = Settings { download_dir: out.to_string_lossy().into_owned(), ..Default::default() };
        let url = "https://www.youtube.com/watch?v=jNQXAC9IVRw";
        let run = |format, clip, on_event: &mut dyn FnMut(Event)| {
            download(&ytdlp, url, Platform::Youtube, format, clip, &settings, Some(&ffmpeg), Some(&deno), &Stopper::default(), on_event)
        };

        let mut converting = false;
        let saved = run(Format::Mp3, None, &mut |e| converting |= matches!(e, Event::Converting)).expect("download");
        assert!(converting, "never reported the MP3 conversion step");
        assert!(!saved.already_saved);
        assert_eq!(saved.files.len(), 1);
        assert!(saved.files[0].ends_with(".mp3"), "{saved:?}");
        println!("saved {}", saved.files[0]);

        // The same video from the MP4 box: merged into an mp4 (needs ffmpeg and Deno for YouTube).
        let video = run(Format::Mp4, None, &mut |_| {}).expect("mp4 download");
        assert_eq!(video.files.len(), 1);
        assert!(video.files[0].ends_with(".mp4"), "{video:?}");
        println!("saved {}", video.files[0]);

        // Both again: found in the folder, nothing downloaded or converted, nothing left behind.
        for (format, first) in [(Format::Mp3, &saved), (Format::Mp4, &video)] {
            let before = std::fs::read_dir(&out).unwrap().count();
            let mut progressed = false;
            let mut dests = Vec::new();
            let again = run(format, None, &mut |e| match e {
                Event::Progress { .. } | Event::Converting => progressed = true,
                Event::Destination(d) => dests.push(d),
                _ => {}
            })
            .expect("already saved");
            assert!(again.already_saved, "{format:?} downloaded again");
            assert!(!progressed, "{format:?} fetched data although the file was there");
            assert_eq!(again.files, first.files);
            // The app tidies up after an already-saved job the same way (e.g. a fetched thumbnail).
            remove_partial_files(&dests, &settings.download_dir);
            assert_eq!(std::fs::read_dir(&out).unwrap().count(), before, "{format:?} left files behind");
        }

        // A clip: its own file, a few seconds long instead of 19.
        let clip = Clip::parse("0:05", "0:08").unwrap();
        let cut = run(Format::Mp3, clip, &mut |_| {}).expect("clip");
        assert!(!cut.already_saved);
        assert!(cut.files[0].contains("(clip 0m05s-0m08s)"), "{cut:?}");
        let whole = std::fs::metadata(&saved.files[0]).unwrap().len();
        let part = std::fs::metadata(&cut.files[0]).unwrap().len();
        assert!(part * 3 < whole, "clip ({part} bytes) isn't much smaller than the whole ({whole} bytes)");
        println!("saved {}", cut.files[0]);
        println!("folder: {:?}", std::fs::read_dir(&out).unwrap().flatten().map(|e| e.file_name()).collect::<Vec<_>>());
    }

    #[test]
    fn parses_clip_times() {
        assert_eq!(parse_time("45"), Some(45));
        assert_eq!(parse_time("1:10"), Some(70));
        assert_eq!(parse_time("01:02:03"), Some(3723));
        for bad in ["", "1:", ":30", "1:60", "1:2:3:4", "-5", "1.5", "a", "1 0", "9999999"] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
        assert_eq!(Clip::parse("", "").unwrap(), None);
        assert_eq!(Clip::parse("0:00", " ").unwrap(), None); // the whole video
        assert_eq!(Clip::parse("0:30", "1:10").unwrap(), Some(Clip { start: 30, end: Some(70) }));
        assert_eq!(Clip::parse("", "1:10").unwrap(), Some(Clip { start: 0, end: Some(70) }));
        assert_eq!(Clip::parse("2:00", "").unwrap(), Some(Clip { start: 120, end: None }));
        assert!(Clip::parse("1:10", "0:30").is_err());
        assert!(Clip::parse("1:10", "1:10").is_err());
        assert!(Clip::parse("soon", "").unwrap_err().contains("soon"));

        let clip = Clip { start: 30, end: Some(3725) };
        assert_eq!(clip.section(), "*30-3725");
        assert_eq!(clip.file_suffix(), " (clip 0m30s-1h02m05s)");
        assert_eq!(Clip { start: 90, end: None }.section(), "*90-inf");
        assert_eq!(Clip { start: 90, end: None }.file_suffix(), " (clip 1m30s-end)");
    }

    #[test]
    fn quality_and_clip_shape_the_yt_dlp_options() {
        let tool = Path::new(r"C:\tools\x.exe");
        let args_for = |quality, clip, ffmpeg| -> Vec<String> {
            let settings = Settings { video_quality: quality, ..test_settings() };
            build_args(Platform::Youtube, Format::Mp4, clip, &settings, ffmpeg, Some(tool))
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        let value = |args: &[String], flag: &str| args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone());

        let f720 = value(&args_for(VideoQuality::P720, None, Some(tool)), "-f").unwrap();
        assert!(f720.starts_with("bv*[vcodec^=avc1][height<=720]+"), "{f720}");
        assert!(!f720.contains("1080"));
        let f1080 = value(&args_for(VideoQuality::P1080, None, Some(tool)), "-f").unwrap();
        assert!(f1080.starts_with("bv*[vcodec^=avc1][height<=1080]+"), "{f1080}");
        assert_eq!(value(&args_for(VideoQuality::Best, None, Some(tool)), "-f").unwrap(), "bv*+ba/b");
        // Without ffmpeg the cap still applies to single-file formats.
        assert!(value(&args_for(VideoQuality::P720, None, None), "-f").unwrap().starts_with("b[height<=720]"));

        let plain = args_for(VideoQuality::P1080, None, Some(tool));
        assert!(!plain.iter().any(|a| a == "--download-sections"));
        let clip = Some(Clip { start: 30, end: Some(70) });
        let cut = args_for(VideoQuality::P1080, clip, Some(tool));
        assert_eq!(value(&cut, "--download-sections").unwrap(), "*30-70");
        assert_eq!(value(&cut, "-o").unwrap(), "%(title).90s (clip 0m30s-1m10s) (%(id)s).%(ext)s");
        assert!(Format::Mp4.needs_ffmpeg(Platform::Twitter, clip));
        assert!(!Format::Mp4.needs_ffmpeg(Platform::Twitter, None));
    }

    #[test]
    fn finds_a_file_saved_by_an_earlier_download() {
        let base = std::env::temp_dir().join(format!("clipdrop_dupe_{}", std::process::id()));
        let dir = base.join("downloads");
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_string_lossy().into_owned();
        let video = dir.join("Song (abc).mp4");
        let song = dir.join("Song [abc].mp3");
        let planned_mp3 = dir.join("Song [abc].webm").to_string_lossy().into_owned();
        let planned_mp4 = video.to_string_lossy().into_owned();

        assert_eq!(finished_file(&planned_mp4, Format::Mp4, &d), None);
        assert_eq!(finished_file(&planned_mp3, Format::Mp3, &d), None);
        std::fs::write(&video, b"x").unwrap();
        std::fs::write(&song, b"x").unwrap();
        assert!(finished_file(&planned_mp4, Format::Mp4, &d).unwrap().ends_with("Song (abc).mp4"));
        // An MP3 job plans the source's extension; the finished file is the .mp3.
        assert!(finished_file(&planned_mp3, Format::Mp3, &d).unwrap().ends_with("Song [abc].mp3"));
        // A file outside the download folder doesn't count.
        let outside = base.join("Song (abc).mp4");
        std::fs::write(&outside, b"x").unwrap();
        assert_eq!(finished_file(&outside.to_string_lossy(), Format::Mp4, &d), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn removes_partial_files_but_keeps_finished_ones() {
        let dir = std::env::temp_dir().join(format!("clipdrop_partial_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stem = "youtube_user_abc";
        let partial = [
            "youtube_user_abc.mp4.part",
            "youtube_user_abc.f137.mp4",
            "youtube_user_abc.f137.mp4.part",
            "youtube_user_abc.f140.m4a.ytdl",
            "youtube_user_abc.temp.mp4",
            "youtube_user_abc.webp",
        ];
        let kept = ["youtube_user_abc.mp4", "youtube_user_abcd.mp4.part", "other.mp4.part"];
        for name in partial.iter().chain(&kept) {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let dest = dir.join(format!("{stem}.mp4")).to_string_lossy().into_owned();
        remove_partial_files(std::slice::from_ref(&dest), &dir.to_string_lossy());
        for name in partial {
            assert!(!dir.join(name).exists(), "{name} should be deleted");
        }
        for name in kept {
            assert!(dir.join(name).exists(), "{name} should be kept");
        }
        // A destination outside the download folder is ignored.
        let outside = std::env::temp_dir().join(format!("clipdrop_outside_{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("x.mp4.part"), b"x").unwrap();
        remove_partial_files(&[outside.join("x.mp4").to_string_lossy().into_owned()], &dir.to_string_lossy());
        assert!(outside.join("x.mp4.part").exists());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }

    /// The stopper must end one download's process tree (like yt-dlp and its ffmpeg) and leave
    /// other downloads running.
    #[cfg(windows)]
    #[test]
    fn stopper_ends_only_its_own_process_tree() {
        let _serial = JOB_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let system32 = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        let ping = system32.join("PING.EXE").to_string_lossy().into_owned();
        let spawn = || {
            let child = command(system32.join("cmd.exe"))
                .args(["/C", &ping, "-n", "60", "127.0.0.1"])
                .stdout(Stdio::null())
                .spawn()
                .unwrap();
            let stopper = Stopper::default();
            stopper.attach(contain(&child));
            (child, stopper)
        };
        let (mut target, stopper) = spawn();
        let (mut other, other_stopper) = spawn();
        assert!(matches!(stopper.lock().killer, Some(Killer::Job(_))), "no per-download job");
        std::thread::sleep(Duration::from_millis(500));
        stopper.stop();
        let started = std::time::Instant::now();
        while target.try_wait().unwrap().is_none() {
            assert!(started.elapsed() < Duration::from_secs(5), "stopped process survived");
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(other.try_wait().unwrap().is_none(), "stopping one download ended another");
        other_stopper.stop();
        other.wait().unwrap();

        // A stop that arrives before the process is attached still ends it.
        let early = Stopper::default();
        early.stop();
        let mut late = command(system32.join("cmd.exe"))
            .args(["/C", &ping, "-n", "60", "127.0.0.1"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        early.attach(contain(&late));
        let started = std::time::Instant::now();
        while late.try_wait().unwrap().is_none() {
            assert!(started.elapsed() < Duration::from_secs(5), "late-attached process survived");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Network test: stops a YouTube MP4 download partway, checks partial files exist, resumes it
    /// to completion, then stops another and checks the cleanup. Needs ffmpeg/Deno like the test above.
    /// Run with `CLIPDROP_TEST_DIR=<dir> cargo test pause_resume -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn pause_resume_and_stop_end_to_end() {
        let ytdlp = real_ytdlp().expect("no yt-dlp found (set CLIPDROP_YTDLP)");
        let base = PathBuf::from(std::env::var_os("CLIPDROP_TEST_DIR").expect("set CLIPDROP_TEST_DIR"));
        let bin_dir = base.join("bin");
        let ffmpeg = find_ffmpeg(&bin_dir).map_or_else(|| install_ffmpeg(&bin_dir), Ok).expect("ffmpeg");
        let deno = find_deno(&bin_dir).map_or_else(|| install_deno(&bin_dir), Ok).expect("deno");
        let out = base.join("pause");
        let _ = std::fs::remove_dir_all(&out);
        let settings = Settings { download_dir: out.to_string_lossy().into_owned(), ..Default::default() };
        // A few minutes long, so it is still downloading when stopped.
        let url = "https://www.youtube.com/watch?v=aqz-KE-bpKQ";

        let run = |stop_at: Option<f64>| {
            let stopper = Stopper::default();
            let mut dests = Vec::new();
            let result = download(&ytdlp, url, Platform::Youtube, Format::Mp4, None, &settings, Some(&ffmpeg), Some(&deno), &stopper, |e| match e {
                Event::Destination(d) => dests.push(d),
                Event::Progress { downloaded, total: Some(total), .. }
                    if stop_at.is_some_and(|at| downloaded / total >= at) =>
                {
                    stopper.stop();
                }
                _ => {}
            });
            (result, dests)
        };
        let leftovers = || std::fs::read_dir(&out).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();

        // Pause: stopped partway, partial files stay for the resume.
        let (result, dests) = run(Some(0.2));
        assert_eq!(result.unwrap_err().message, STOPPED);
        assert_eq!(dests.len(), 1, "{dests:?}");
        println!("after pause: {:?}", leftovers());
        assert!(leftovers().iter().any(|n| n.contains(".part")), "no partial file to resume from");

        // Resume finishes it.
        let (result, _) = run(None);
        let files = result.expect("resumed download").files;
        assert!(files[0].ends_with(".mp4"));
        std::fs::remove_file(&files[0]).unwrap();

        // Stop: partial files are removed.
        let (result, dests) = run(Some(0.2));
        assert_eq!(result.unwrap_err().message, STOPPED);
        remove_partial_files(&dests, &settings.download_dir);
        assert!(leftovers().is_empty(), "left behind: {:?}", leftovers());
    }
}
