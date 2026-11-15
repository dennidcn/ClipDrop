"""Regenerates the preview pages from src/index.html.

Each preview page loads the real ../src/styles.css and ../src/main.js, plus mock-tauri.js
with a data-mode that picks the state to show. Run from anywhere: python preview/build.py
"""
from pathlib import Path

HERE = Path(__file__).parent
MODES = ["list", "empty", "settings", "installing", "error"]

src = (HERE.parent / "src" / "index.html").read_text(encoding="utf-8")
for mode in MODES:
    html = src.replace('href="styles.css"', 'href="../src/styles.css"')
    html = html.replace(
        '<script type="module" src="main.js" defer></script>',
        f'<script src="mock-tauri.js" data-mode="{mode}"></script>\n'
        '    <script src="../src/main.js" defer></script>',
    )
    html = html.replace("<title>ClipDrop</title>", f"<title>ClipDrop preview: {mode}</title>")
    html = html.replace("<body>", '<body style="height:680px;max-width:460px;margin:0 auto">', 1)
    (HERE / f"{mode}.html").write_text(html, encoding="utf-8")
    print(f"wrote preview/{mode}.html")
