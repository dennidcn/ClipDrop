<img src="assets/app-icon.svg" width="64" height="64" alt="">

# ClipDrop

Copy a link, get the video. ClipDrop is a small desktop app that watches your clipboard. When you copy a link to an **X / Twitter**, **Instagram** or **TikTok** post, it downloads the video to a folder.

- **Tauri v2** (Rust backend plus the system webview). Release builds are a few MB, not the ~150 MB an Electron app would be.
- Downloads run through [yt-dlp](https://github.com/yt-dlp/yt-dlp). It is fetched automatically on first launch and kept in the app's data folder.
- Closing the window minimises the app to the tray, where it keeps watching. Use **Quit** in the tray menu to exit.

## Supported links

| Platform    | Examples |
|-------------|----------|
| X / Twitter | `x.com/<user>/status/<id>`, `twitter.com/...`, `mobile.twitter.com/...`, `fxtwitter` / `vxtwitter` / `fixupx` |
| Instagram   | `/reel/<code>`, `/reels/<code>`, `/p/<code>`, `/tv/<code>` |
| TikTok      | `tiktok.com/@<user>/video/<id>`, `vm.tiktok.com/<code>`, `vt.tiktok.com/<code>`, `tiktok.com/t/<code>` |

Links are normalised, so copying the same post twice (for example the `twitter.com` and `x.com` versions) only downloads it once.

## Development

Requirements: [Rust](https://rustup.rs) and Node.js (Node is only needed for the Tauri CLI; the frontend is plain HTML, CSS and JS with no bundler).

```sh
npm install
npm run dev      # run the app with hot reload of the Rust side
npm run build    # release build + installer in src-tauri/target/release/bundle
cd src-tauri && cargo test   # link-detection tests
```

## Layout

```
src/                     UI (index.html, main.js, styles.css)
src-tauri/src/lib.rs     app state, job queue, clipboard watcher, tray, Tauri commands
src-tauri/src/links.rs   URL detection and normalisation (+ tests)
src-tauri/src/downloader.rs  yt-dlp install / run / progress parsing
src-tauri/src/settings.rs    settings persisted as JSON in the app config dir
```

## Notes

- **ffmpeg (optional).** If `ffmpeg` is on your `PATH`, ClipDrop downloads the best separate video and audio streams and merges them into an mp4. Without it, it picks the best single-file format, which works for nearly every post on these three sites.
- **Instagram login.** Some posts need you to be logged in. In Settings, choose a browser under **Cookies** so yt-dlp can use that browser's session. Firefox works most reliably. Chrome and Edge may need to be closed first because they lock their cookie database.
- **When a site breaks.** The platforms change often. Click **Update yt-dlp** in Settings first; most breakages are fixed upstream within days.
- Only download content you have the right to save.
