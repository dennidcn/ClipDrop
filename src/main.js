const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const appWindow = window.__TAURI__.window.getCurrentWindow();

const $ = (id) => document.getElementById(id);

const jobs = new Map(); // id -> job
const rows = new Map(); // id -> <li>
let settings = null;

const PLATFORM = { twitter: "X", instagram: "Instagram", tiktok: "TikTok" };

// Segoe Fluent Icons code points
const G = {
  link: "",
  download: "",
  done: "",
  error: "",
  open: "",
  folder: "",
  retry: "",
  remove: "",
  info: "",
  maximize: "",
  restore: "",
};

// ---------- helpers ----------

function formatSpeed(bps) {
  if (!bps) return "";
  const units = ["B/s", "KB/s", "MB/s", "GB/s"];
  let i = 0;
  while (bps >= 1024 && i < units.length - 1) { bps /= 1024; i++; }
  return `${bps.toFixed(bps < 10 && i > 0 ? 1 : 0)} ${units[i]}`;
}

const fileName = (path) => (path ? path.split(/[\\/]/).pop() : "");

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") node.className = v;
    else if (k === "text") node.textContent = v;
    else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else node.setAttribute(k, v);
  }
  for (const c of children) if (c != null) node.append(c);
  return node;
}

function iconButton(glyph, label, onclick) {
  return el("button", { class: "btn subtle icon", "aria-label": label, title: label, onclick },
    el("span", { class: "glyph", text: glyph }));
}

// ---------- downloads list ----------

function jobTitle(job) {
  return job.title || job.url.replace(/^https?:\/\/(www\.)?/, "");
}

function jobMeta(job) {
  const platform = PLATFORM[job.platform];
  switch (job.status) {
    case "detected": return `${platform} · Waiting for you to start it`;
    case "downloading": {
      const pct = job.progress > 0 ? `${Math.round(job.progress * 100)}%` : "Starting";
      return [platform, pct, formatSpeed(job.speed)].filter(Boolean).join(" · ");
    }
    case "done": return `${platform} · ${fileName(job.file) || "Saved"}`;
    case "error": return job.error || "The download failed.";
  }
}

function jobActions(job) {
  const id = job.id;
  const run = (cmd) => () => invoke(cmd, { id }).catch((e) => console.error(e));
  const actions = [];
  switch (job.status) {
    case "detected":
      actions.push(el("button", { class: "btn small", onclick: run("start_download"), text: "Download" }));
      break;
    case "done":
      actions.push(iconButton(G.open, "Open video", run("open_file")));
      actions.push(iconButton(G.folder, "Show in folder", run("reveal_file")));
      break;
    case "error":
      actions.push(iconButton(G.retry, "Try again", run("start_download")));
      break;
  }
  if (job.status !== "downloading") {
    actions.push(iconButton(G.remove, "Remove from list", () => removeJob(id)));
  }
  return actions;
}

const TILE = { detected: G.link, downloading: G.download, done: G.done, error: G.error };

function buildRow(job) {
  const li = el("li", { class: "job" },
    el("span", { class: "job-tile glyph", "aria-hidden": "true" }),
    el("div", { class: "job-body" },
      el("span", { class: "job-title" }),
      el("span", { class: "job-meta" }),
    ),
    el("div", { class: "job-actions" }),
  );
  rows.set(job.id, li);
  return li;
}

/** Updates a row in place so hover and focus survive frequent progress events. */
function patchRow(li, job) {
  const statusChanged = li.dataset.status !== job.status;
  li.className = `job ${job.status}`;
  li.title = job.url;
  li.querySelector(".job-tile").textContent = TILE[job.status];
  li.querySelector(".job-title").textContent = jobTitle(job);
  li.querySelector(".job-meta").textContent = jobMeta(job);

  let bar = li.querySelector(".progress");
  if (job.status === "downloading") {
    if (!bar) {
      bar = el("div", { class: "progress", role: "progressbar", "aria-label": "Download progress" },
        el("div", { class: "progress-bar" }));
      li.querySelector(".job-body").append(bar);
    }
    const known = job.progress > 0;
    bar.classList.toggle("indeterminate", !known);
    bar.firstChild.style.width = `${Math.round(job.progress * 100)}%`;
    if (known) bar.setAttribute("aria-valuenow", Math.round(job.progress * 100));
    else bar.removeAttribute("aria-valuenow");
  } else if (bar) {
    bar.remove();
  }

  if (statusChanged) {
    li.dataset.status = job.status;
    li.querySelector(".job-actions").replaceChildren(...jobActions(job));
  }
}

function render() {
  const list = [...jobs.values()].sort((a, b) => b.id - a.id);
  const ul = $("jobs");

  for (const [id, li] of rows) {
    if (!jobs.has(id)) { li.remove(); rows.delete(id); }
  }
  list.forEach((job, index) => {
    const li = rows.get(job.id) ?? buildRow(job);
    patchRow(li, job);
    if (ul.children[index] !== li) ul.insertBefore(li, ul.children[index] ?? null);
  });

  const active = list.filter((j) => j.status === "downloading").length;
  const finished = list.length - active;
  $("empty").hidden = list.length > 0;
  $("list-head").hidden = list.length === 0;
  $("count").textContent = active ? `Downloading ${active}` : "Recent";
  $("clear").hidden = finished === 0;
}

async function removeJob(id) {
  await invoke("remove_job", { id });
  jobs.delete(id);
  render();
}

// ---------- settings ----------

function syncToggle(input, checked) {
  input.checked = checked;
  input.closest(".toggle").querySelector(".toggle-state").textContent = checked ? "On" : "Off";
}

function renderSettings() {
  syncToggle($("watch"), settings.watchClipboard);
  syncToggle($("auto"), settings.autoDownload);
  syncToggle($("notify"), settings.notify);
  $("watch-desc").textContent = settings.watchClipboard
    ? settings.autoDownload
      ? "Copied post links download automatically."
      : "Copied post links are added to the list for you to start."
    : "Paused. Copied links are ignored.";
  $("cookies").value = settings.cookiesBrowser;
  $("folder-path").textContent = settings.downloadDir;
  $("folder-path").title = settings.downloadDir;
}

async function saveSettings(patch) {
  settings = await invoke("save_settings", { settings: { ...settings, ...patch } });
  renderSettings();
}

const TOOL = {
  installing: { cls: "", title: "Getting ready.", msg: "Downloading the video engine. This only happens once." },
  error: { cls: "error", title: "Can't download videos yet.", msg: "The video engine didn't install. Check your connection." },
};

function renderToolStatus(status) {
  const t = TOOL[status];
  $("tool-bar").hidden = !t;
  if (!t) return;
  $("tool-bar").className = `infobar ${t.cls}`;
  $("tool-bar").querySelector(".infobar-icon").textContent = status === "error" ? "" : "";
  $("tool-title").textContent = t.title;
  $("tool-msg").textContent = t.msg;
  $("tool-retry").hidden = status !== "error";
}

// ---------- navigation ----------

function show(pageId) {
  for (const page of document.querySelectorAll(".page")) {
    const on = page.id === pageId;
    page.hidden = !on;
    page.classList.toggle("enter", on);
  }
  const inSettings = pageId === "page-settings";
  $("back").hidden = !inSettings;
  document.querySelector(".titlebar").classList.toggle("has-back", inSettings);
  (inSettings ? $("settings-title") : $("open-settings")).focus({ preventScroll: true });
}

/** Hides the WebView2 browser shell (context menu, reload, print, find) so the window reads as an app. */
function suppressBrowserChrome() {
  document.addEventListener("contextmenu", (e) => {
    if (!e.target.closest("input, textarea, [contenteditable]")) e.preventDefault();
  });
  document.addEventListener("keydown", (e) => {
    const k = e.key.toLowerCase();
    const mod = e.ctrlKey || e.metaKey;
    if (k === "f5" || (mod && ["r", "p", "f", "g", "u", "j"].includes(k)) || (mod && e.shiftKey && k === "i")) {
      e.preventDefault();
    }
  });
}

function showAddError(msg) {
  $("add-error").textContent = msg || "";
  $("add-error").hidden = !msg;
}

// ---------- window frame ----------

async function syncMaximize() {
  const max = await appWindow.isMaximized();
  const btn = $("win-max");
  btn.querySelector(".glyph").textContent = max ? G.restore : G.maximize;
  btn.title = max ? "Restore" : "Maximize";
  btn.setAttribute("aria-label", btn.title);
}

function initFrame(mica) {
  document.documentElement.classList.toggle("mica", mica);
  $("win-min").addEventListener("click", () => appWindow.minimize());
  $("win-max").addEventListener("click", () => appWindow.toggleMaximize());
  $("win-close").addEventListener("click", () => appWindow.close());
  appWindow.onResized(syncMaximize);
  syncMaximize();
  window.addEventListener("blur", () => document.body.classList.add("inactive"));
  window.addEventListener("focus", () => document.body.classList.remove("inactive"));
}

// ---------- boot ----------

async function init() {
  suppressBrowserChrome();
  const snap = await invoke("get_snapshot");
  console.info("[ClipDrop] Mica backdrop applied:", snap.mica);
  initFrame(snap.mica);
  settings = snap.settings;
  for (const job of snap.jobs) jobs.set(job.id, job);
  renderSettings();
  renderToolStatus(snap.toolStatus);
  render();

  await listen("job-updated", ({ payload }) => {
    jobs.set(payload.id, payload);
    render();
  });
  await listen("tool-status", ({ payload }) => renderToolStatus(payload));

  $("add-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const text = $("add-input").value.trim();
    if (!text) return;
    try {
      await invoke("add_url", { text });
      $("add-input").value = "";
      showAddError("");
    } catch (err) {
      showAddError(String(err));
    }
  });
  $("add-input").addEventListener("input", () => showAddError(""));

  $("watch").addEventListener("change", (e) => saveSettings({ watchClipboard: e.target.checked }));
  $("auto").addEventListener("change", (e) => saveSettings({ autoDownload: e.target.checked }));
  $("notify").addEventListener("change", (e) => saveSettings({ notify: e.target.checked }));
  $("cookies").addEventListener("change", (e) => saveSettings({ cookiesBrowser: e.target.value }));

  $("folder").addEventListener("click", async () => {
    const dir = await invoke("pick_folder");
    if (dir) saveSettings({ downloadDir: dir });
  });
  $("open-folder").addEventListener("click", () => invoke("open_download_dir").catch(console.error));

  $("clear").addEventListener("click", async () => {
    const remaining = await invoke("clear_finished");
    jobs.clear();
    for (const job of remaining) jobs.set(job.id, job);
    render();
  });

  $("open-settings").addEventListener("click", () => show("page-settings"));
  $("back").addEventListener("click", () => show("page-downloads"));
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("page-settings").hidden) show("page-downloads");
  });

  const runUpdate = async (button, descEl) => {
    button.disabled = true;
    const original = button.textContent;
    button.textContent = "Checking…";
    try {
      const msg = await invoke("update_ytdlp");
      if (descEl) descEl.textContent = msg.split("\n").filter(Boolean).pop() ?? "Up to date.";
    } catch (err) {
      if (descEl) descEl.textContent = String(err).split("\n").filter(Boolean).pop();
    } finally {
      button.textContent = original;
      button.disabled = false;
    }
  };
  $("update").addEventListener("click", () => runUpdate($("update"), $("engine-desc")));
  $("tool-retry").addEventListener("click", () => runUpdate($("tool-retry"), null));
}

init().catch((err) => {
  console.error(err);
  showAddError(`ClipDrop couldn't start: ${err}`);
});
