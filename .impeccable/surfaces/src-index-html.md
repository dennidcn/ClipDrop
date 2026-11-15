---
version: 1
slug: "src-index-html"
primary_target: "src/index.html"
related_targets: ["src/styles.css","src/main.js"]
---

# Surface: ClipDrop main window (frame + shell)

Scope: the whole app window: custom title bar, Downloads page, Settings page. Mode: **Operate**. The window is glanced at for a few seconds while the app mostly runs in the tray.

Audience/job: the author, saving clips casually. They check that a clip landed, open it, fix a failure, or change a setting.

Constraints: Windows only. Tauri webview, local assets only (CSP self). Must keep every existing function: watch toggle, paste box, job states (detected, downloading, done, error), open/show/retry/remove, clear finished, settings (folder, auto, notify, cookies, update yt-dlp).

## Direction contract

THESIS: ClipDrop as a first-party Windows 11 utility, the category standard played straight at PowerToys craft. No metaphor, no themed world. It refuses the generic web-app look (a web header, a centered input, a footer drawer) and replaces it with genuine Fluent window anatomy.

OWN-WORLD: Mica backdrop (solid #f3f3f3 / #202020 fallback), translucent layer cards with 1px hairline strokes at 4–8px radius, Segoe UI Variable on the Fluent type ramp (Caption 12, Body 14, Subtitle 20, Title 28), Segoe Fluent Icons, Windows default accent (#005fb8 light, #60cdff dark), Fluent ToggleSwitch, ProgressBar, accent and standard buttons, ComboBox, InfoBar. It follows the system light/dark setting.

STORY: open the window, see at once that the app is watching and what arrived, then act on one item or close. Settings sit one click away as a settings-card page, like PowerToys.

FIRST VIEWPORT: a 40px custom title bar (16px app icon, "ClipDrop" in Caption, caption buttons 46×40 at the right with a red close hover, the rest draggable). Then a "Downloads" Title heading with a settings button at the right. Then a settings card "Watch clipboard" with a description and a ToggleSwitch. Then a link TextBox with an accent "Download" button. Then the download list as stacked list cards (state glyph tile, title, platform · size/speed meta, inline progress bar, subtle action buttons), newest first. Primary action: the Download button / copying.

FORM: category standard (canon), chosen by the user over the roll; quality bar = PowerToys. seed:c915961197cd76f9

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
