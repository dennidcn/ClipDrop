<img src="assets/app-icon.svg" width="64" height="64" alt="">

# ClipDrop

Paste a link, get the file. ClipDrop is a small desktop app with two boxes: paste an **X / Twitter**, **Instagram**, **TikTok** or **YouTube** link into **Video (MP4)** to save the video, or into **Audio (MP3)** to save just the audio, with the title, artist and thumbnail as cover art.

- **Tauri v2** (Rust backend plus the system webview). Release builds are a few MB, not the ~150 MB an Electron app would be.
- Downloads run through [yt-dlp](https://github.com/yt-dlp/yt-dlp). It is fetched automatically on first launch and kept in the app's data folder.
- Closing the window quits ClipDrop. If downloads are still running it asks first, and quitting stops them. The tray icon offers **Check for updates**.

## Supported links

| Platform    | Examples |
|-------------|----------|
| X / Twitter | `x.com/<user>/status/<id>`, `twitter.com/...`, `mobile.twitter.com/...`, `fxtwitter` / `vxtwitter` / `fixupx` |
| Instagram   | `/reel/<code>`, `/reels/<code>`, `/p/<code>`, `/tv/<code>` |
| TikTok      | `tiktok.com/@<user>/video/<id>`, `vm.tiktok.com/<code>`, `vt.tiktok.com/<code>`, `tiktok.com/t/<code>` |
| YouTube (MP3) | `youtube.com/watch?v=<id>`, `youtu.be/<id>`, `/shorts/<id>`, `/live/<id>`, `m.youtube.com`, `music.youtube.com` |

Links are normalised, so pasting the same post twice (for example the `twitter.com` and `x.com` versions) only downloads it once.

## Development

Requirements: [Rust](https://rustup.rs) and Node.js (Node is only needed for the Tauri CLI; the frontend is plain HTML, CSS and JS with no bundler).

```sh
npm install
npm run dev      # run the app with hot reload of the Rust side
npm run build    # release build + installer in src-tauri/target/release/bundle
cd src-tauri && cargo test   # link-detection tests
```

## Releasing and updates

ClipDrop updates itself. It checks this repo's latest GitHub release shortly after launch and every 12 hours, and the tray menu has **Check for updates**. An update is only installed if it is signed with the release key, whose public half is in `src-tauri/tauri.conf.json`.

1. Bump `version` in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `package.json`.
2. `npm run release -- --notes "What changed"`. This builds the installer, signs it with the key in `~/.tauri/clipdrop.key` (it asks for the key's password), and writes `latest.json` next to it. Add `--publish` to create the GitHub release with `gh`.
3. Without `--publish`, create a release tagged `v<version>` and attach `ClipDrop_<version>_x64-setup.exe` and `latest.json`.

Keep a backup of `~/.tauri/clipdrop.key` and its password. Without them, installed copies can't be updated.

## Layout

```
src/                     UI (index.html, main.js, styles.css)
src-tauri/src/lib.rs     app state, job queue, tray, Tauri commands
src-tauri/src/links.rs   URL detection and normalisation (+ tests)
src-tauri/src/downloader.rs  yt-dlp install / run / progress parsing
src-tauri/src/settings.rs    settings persisted as JSON in the app config dir
```

## Notes

- **ffmpeg.** MP3s and YouTube videos need ffmpeg (YouTube keeps its good video and audio as separate streams to merge). ClipDrop uses one on your `PATH` if there is one. Otherwise, the first time it's needed, it downloads the [yt-dlp project's ffmpeg build](https://github.com/yt-dlp/FFmpeg-Builds) (about 200 MB, checked against the release's SHA-256) into its data folder. Videos are saved as H.264 mp4 (up to 1080p) so they play everywhere. X, Instagram and TikTok videos use ffmpeg when it's available to merge the best separate streams. Without it, they pick the best single-file format, which works for nearly every post on X, Instagram and TikTok.
- **YouTube and Deno.** YouTube makes yt-dlp run some JavaScript to unlock its formats. ClipDrop uses [Deno](https://deno.com) for this (yt-dlp's default runtime, which runs that code with no file or network access): one on your `PATH` if there is one, otherwise it downloads the official build (about 45 MB, checked against its published SHA-256) alongside ffmpeg on the first YouTube link. If that download fails, YouTube still works, possibly with fewer formats, and it tries again next launch.
- **Instagram login.** Some posts need you to be logged in. In Settings, choose a browser under **Cookies** so yt-dlp can use that browser's session. Firefox works most reliably. Chrome and Edge may need to be closed first because they lock their cookie database.
- **When a site breaks.** The platforms change often. Click **Update yt-dlp** in Settings first; most breakages are fixed upstream within days.
- Only download content you have the right to save.
