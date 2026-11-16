<img src="assets/app-icon.svg" width="64" height="64" alt="">

# ClipDrop

Paste a link, get the file. ClipDrop is a small Windows app for saving posts from **X / Twitter**, **Instagram**, **TikTok** and **YouTube**, either as a video (MP4) or as audio only (MP3).

## Download

Get `ClipDrop_<version>_x64-setup.exe` from the [latest release](https://github.com/dennidcn/twitter-video-downloader/releases/latest) and run it. ClipDrop runs on Windows 10 and 11 and updates itself after that.

There's nothing else to install. ClipDrop fetches the tools it needs the first time it needs them:

- **yt-dlp** (about 24 MB) on first launch. It does the downloading.
- **ffmpeg** and **Deno** (about 250 MB together) the first time you save an MP3 or a YouTube video.

Each one is checked against its published SHA-256 before it's used.

## Using it

Copy a link, then paste it into one of the two boxes and press **Download**:

| Box | What you get |
|-----|--------------|
| **Video (MP4)** | The video as an mp4 (H.264, up to 1080p), which plays everywhere. |
| **Audio (MP3)** | Just the audio as an MP3, named after the title, with title, artist and thumbnail as cover art. |

Both boxes take links from any supported site, so you can grab a TikTok's sound as an MP3 or a YouTube video as an MP4. The same post can be saved both ways. Files go to `Downloads\ClipDrop`; you can change this in Settings.

Closing the window quits ClipDrop. If something is still downloading it asks first, and quitting stops those downloads. The tray icon has **Check for updates**.

## Supported links

| Site        | Examples |
|-------------|----------|
| X / Twitter | `x.com/<user>/status/<id>`, `twitter.com/...`, `mobile.twitter.com/...`, `fxtwitter` / `vxtwitter` / `fixupx` |
| Instagram   | `/reel/<code>`, `/reels/<code>`, `/p/<code>`, `/tv/<code>` |
| TikTok      | `tiktok.com/@<user>/video/<id>`, `vm.tiktok.com/<code>`, `vt.tiktok.com/<code>`, `tiktok.com/t/<code>` |
| YouTube     | `youtube.com/watch?v=<id>`, `youtu.be/<id>`, `/shorts/<id>`, `/live/<id>`, `m.youtube.com`, `music.youtube.com` |

Links are normalised, so pasting the same post twice into the same box (for example the `twitter.com` and `x.com` versions) only downloads it once. Playlists, channels and live streams aren't downloaded.

## Troubleshooting

- **Instagram says you need to log in.** In Settings, turn on **Use browser sign-in** and pick the browser you're logged in with, so yt-dlp can use that session. Firefox works most reliably; close Chrome or Edge first, because they lock their cookie database. This setting applies to every site, including YouTube, so leave it on **None** unless you need it.
- **A site stopped working.** These sites change often. In Settings, click **Check for updates** under **Downloader engine** first; most breakages are fixed in yt-dlp within days.
- **YouTube errors like "HTTP Error 403" or "confirm you're not a bot".** YouTube sometimes limits connections that download a lot. Wait a bit and press retry.
- Only download content you have the right to save.

## Development

Requirements: [Rust](https://rustup.rs) and Node.js. Node is only needed for the Tauri CLI; the frontend is plain HTML, CSS and JS with no bundler. ClipDrop is built with **Tauri v2** (Rust backend plus the system webview), so the installer is a few MB rather than the ~150 MB an Electron app would be.

```sh
npm install
npm run dev      # run the app
npm run build    # release build + installer in src-tauri/target/release/bundle
cd src-tauri && cargo test   # unit tests; also checks our options against the installed yt-dlp
```

There's also a network end-to-end test, off by default. It installs ffmpeg and Deno into a folder and saves a short YouTube video as MP3 and MP4:

```sh
cd src-tauri && CLIPDROP_TEST_DIR=<folder> cargo test youtube_to_mp3 -- --ignored --nocapture
```

The `preview/` pages show the UI in a browser with mock data; regenerate them with `python preview/build.py` after editing `src/index.html`.

## Releasing and updates

ClipDrop updates itself. It checks this repo's latest GitHub release shortly after launch and every 12 hours. An update is only installed if it is signed with the release key, whose public half is in `src-tauri/tauri.conf.json`.

1. Bump the version in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `package.json` and the settings footer in `src/index.html`.
2. Run `npm run release -- --notes "What changed" --publish`. This builds the installer, signs it with the key in `~/.tauri/clipdrop.key`, writes `latest.json`, and creates the GitHub release with `gh`. It asks for the key's password, or reads it from `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. `gh` must be logged in with permission to write the repo's contents.
3. Without `--publish`, create a release tagged `v<version>` yourself and attach `ClipDrop_<version>_x64-setup.exe` and `latest.json`.

Keep a backup of `~/.tauri/clipdrop.key` and its password. Without them, installed copies can't be updated.

## Layout

```
src/                         UI (index.html, main.js, styles.css)
src-tauri/src/lib.rs         app state, job queue, tool setup, tray, Tauri commands
src-tauri/src/links.rs       URL detection and normalisation (+ tests)
src-tauri/src/downloader.rs  yt-dlp / ffmpeg / Deno install, yt-dlp options, progress parsing
src-tauri/src/settings.rs    settings persisted as JSON in the app config dir
src-tauri/src/updater.rs     signed self-updates from GitHub Releases
```

## How it works

- **yt-dlp** does the downloading. Each site is locked to its own yt-dlp extractors, and yt-dlp config files are ignored, so a stray `yt-dlp.conf` can't change what it does.
- **ffmpeg** converts audio to MP3, embeds the cover art, and merges separate video and audio streams. YouTube needs it for video too, because it keeps good video and audio as separate streams. ClipDrop uses an ffmpeg on your `PATH` if there is one; otherwise it downloads the [yt-dlp project's ffmpeg build](https://github.com/yt-dlp/FFmpeg-Builds). Without ffmpeg, X, Instagram and TikTok videos fall back to the best single-file format.
- **Deno** runs the JavaScript YouTube requires before it will hand over its formats. It's yt-dlp's default runtime and runs that code with no file or network access. If Deno can't be installed, YouTube still works, possibly with fewer formats, and ClipDrop tries again next launch.
- Downloads run in a Windows job object, so yt-dlp and anything it starts stop when ClipDrop exits, including during an update.

## License

[MIT](LICENSE): use, copy, change and share it however you like. ClipDrop downloads [yt-dlp](https://github.com/yt-dlp/yt-dlp), [ffmpeg](https://ffmpeg.org) and [Deno](https://deno.com) at runtime rather than bundling them; each keeps its own license.
