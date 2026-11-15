# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(Tauri v2 desktop app rendering HTML/CSS/JS in the system webview. Ships on **Windows only**, so the window frame follows Windows conventions: caption buttons top-right. The custom frame doesn't yet show the Snap Layouts flyout on maximize hover; that needs a native hit-test layer.)

## Users

The author, using it casually for themselves. They're scrolling X, Instagram or TikTok, see a clip worth keeping, copy the link, and want it saved without thinking about it. The app spends most of its life in the system tray; the window is opened briefly to check that something landed, open the file, or change a setting.

## Product Purpose

ClipDrop turns "copy link" into "video saved". It watches the clipboard and downloads X/Twitter, Instagram and TikTok videos to a folder on its own. Success means the user never opens a downloader website again and never has to babysit a download.

## Positioning

There's no paste-and-click step. The clipboard is the input, and the app works quietly from the tray. Most competitors are websites or paste-a-link tools.

## Operating Context

- Windows desktop, small window (about 440×640, minimum 360×420), often left in the tray.
- The window is glanced at, not worked in. Typical visits last a few seconds.
- Files go to `Downloads\ClipDrop` by default. yt-dlp is installed and updated by the app.

## Capabilities and Constraints

- Detects post links from X/Twitter (including fx/vx mirrors), Instagram (reel, p, tv) and TikTok (full and short links). Duplicate copies of the same post are ignored.
- Job states: detected (auto-download off), downloading (with progress and speed), done (open or show in folder), error (retry).
- Settings: watch clipboard on/off, auto-download, notifications, download folder, browser cookies for logged-in Instagram, update yt-dlp.
- Closing the window hides it to the tray; quit from the tray menu.
- Frontend is plain HTML/CSS/JS with no bundler. Fonts and assets must be served locally (CSP `default-src 'self'`).

## Brand Commitments

- Name: **ClipDrop**.
- App icon: a white download arrow on a violet rounded square (`assets/app-icon.svg`). Not binding; it was a placeholder.
- **Standing preference (chosen 2026-11-15):** ClipDrop looks and behaves like a native Windows 11 Fluent app, at the craft level of **PowerToys**. Conventions are embraced fully rather than given a twist: Mica, Segoe UI Variable, Fluent controls and settings cards, Windows caption buttons.

## Evidence on Hand

No users, metrics or testimonials. Personal tool.

## Product Principles

1. Copying is the interface; the window is a receipt.
2. Never make the user wait or watch. Background-first, glanceable when opened.
3. Failures explain themselves in plain words and offer one obvious fix.
4. Lightweight in every sense: small binary, few controls, no account.
