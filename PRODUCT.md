# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(Tauri v2 desktop app rendering HTML/CSS/JS in the system webview. Ships on **Windows only**, so the window frame follows Windows conventions: caption buttons top-right. The custom frame doesn't yet show the Snap Layouts flyout on maximize hover; that needs a native hit-test layer.)

## Users

The author, using it casually for themselves. They're scrolling X, Instagram or TikTok, see a clip or song worth keeping, copy the link, paste it into ClipDrop, and want it saved without thinking about it. The window is opened briefly to paste a link, check that something landed, open the file, or change a setting.

## Product Purpose

ClipDrop turns "paste link" into "file saved". It saves posts from X/Twitter, Instagram, TikTok and YouTube to a folder, as video (MP4) or audio (MP3) depending on which of its two paste boxes the link goes in. Success means the user never opens a downloader website again and never has to babysit a download.

## Positioning

A native desktop app instead of an ad-laden downloader website: paste a link and the file lands in a folder. Links are only ever taken from what the user pastes; ClipDrop does not read the clipboard on its own (removed 2026-11-16).

## Operating Context

- Windows desktop, small window (about 440×640, minimum 360×420).
- The window is glanced at, not worked in. Typical visits last a few seconds.
- Files go to `Downloads\ClipDrop` by default. yt-dlp is installed and updated by the app.

## Capabilities and Constraints

- Detects post links from X/Twitter (including fx/vx mirrors), Instagram (reel, p, tv) and TikTok (full and short links). Duplicate copies of the same post are ignored.
- Also detects YouTube links (watch, youtu.be, shorts, live, YouTube Music). Two paste boxes, Video (MP4) and Audio (MP3), each accept any supported site; the box decides the format, and the same post can be saved both ways. MP3s get title, artist and cover art. ffmpeg and Deno (yt-dlp's sandboxed JavaScript runtime for YouTube) are fetched once on first use if they aren't on `PATH`; nobody needs Node.js or anything else installed.
- Job states: downloading (with progress and speed, then "converting" for MP3s), paused, stopped, done (open or show in folder; "Already saved" when the file was already in the folder from an earlier session, in which case nothing is downloaded), error (one plain sentence plus at most one fix button, Browser sign-in or Update and retry, next to retry). Pasted links start at once; up to the "Downloads at once" setting run together.
- Optional trim: a scissors toggle next to each box reveals From/To fields so only part of a video is saved (yt-dlp `--download-sections`). Clips are separate jobs and files from the whole video.
- Settings: download folder, video quality (720p, 1080p default, Best), downloads at once (1–5, default 3), notifications, browser cookies for logged-in Instagram, update yt-dlp.
- Closing the window quits the app (with a confirmation if downloads are running, which are then stopped).
- Frontend is plain HTML/CSS/JS with no bundler. Fonts and assets must be served locally (CSP `default-src 'self'`).

## Brand Commitments

- Name: **ClipDrop**.
- App icon: a white download arrow on a violet rounded square (`assets/app-icon.svg`). Not binding; it was a placeholder.
- **Standing preference (chosen 2026-11-15):** ClipDrop looks and behaves like a native Windows 11 Fluent app, at the craft level of **PowerToys**. Conventions are embraced fully rather than given a twist: Mica, Segoe UI Variable, Fluent controls and settings cards, Windows caption buttons.

## Evidence on Hand

No users, metrics or testimonials. Personal tool.

## Product Principles

1. Pasting a link is the whole interface; the rest of the window is a receipt.
2. Never make the user wait or watch. Background-first, glanceable when opened.
3. Failures explain themselves in plain words and offer one obvious fix.
4. Lightweight in every sense: small binary, few controls, no account.
