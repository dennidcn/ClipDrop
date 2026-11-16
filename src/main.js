const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const appWindow = window.__TAURI__.window.getCurrentWindow();

const $ = (id) => document.getElementById(id);

const jobs = new Map(); // id -> job
const rows = new Map(); // id -> <li>
let settings = null;

const PLATFORM = { twitter: "X", instagram: "Instagram", tiktok: "TikTok", youtube: "YouTube" };

// Segoe Fluent Icons code points
const G = {
  download: "",
  done: "",
  error: "",
  open: "",
  folder: "",
  retry: "",
  remove: "",
  info: "",
  pause: "",
  resume: "",
  stop: "",
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

/** Seconds as 1:05 or 1:02:05. */
function clock(s) {
  const h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60, sec = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

/** "Clip 0:30–1:10" for a job that saves only part of the video. */
function clipLabel(clip) {
  if (!clip) return "";
  return `Clip ${clock(clip.start)}–${clip.end == null ? "end" : clock(clip.end)}`;
}

function jobMeta(job) {
  const where = [PLATFORM[job.platform], clipLabel(job.clip)].filter(Boolean).join(" · ");
  switch (job.status) {
    case "downloading": {
      if (job.converting) return `${where} · Converting to MP3`;
      const pct = job.progress > 0 ? `${Math.round(job.progress * 100)}%` : "Starting";
      return [where, pct, formatSpeed(job.speed)].filter(Boolean).join(" · ");
    }
    case "paused": {
      const pct = job.progress > 0 ? `${Math.round(job.progress * 100)}%` : "";
      return [where, "Paused", pct].filter(Boolean).join(" · ");
    }
    case "stopped": return `${where} · Stopped`;
    case "done":
      return [where, job.alreadySaved ? "Already saved" : "", fileName(job.file) || "Saved"].filter(Boolean).join(" · ");
    case "error": return job.error || "The download failed.";
  }
}

/** The one obvious fix offered next to a failed download. */
const FIX = {
  signIn: { label: "Browser sign-in", run: () => showSignIn() },
  update: { label: "Update and retry", run: (id, button) => updateAndRetry(id, button) },
};

function showSignIn() {
  show("page-settings");
  const card = $("cookies-card");
  card.scrollIntoView({ block: "center" });
  $("cookies").focus({ preventScroll: true });
  card.classList.remove("highlight");
  void card.offsetWidth; // restart the highlight animation
  card.classList.add("highlight");
}

async function updateAndRetry(id, button) {
  button.disabled = true;
  button.textContent = "Updating…";
  try {
    await invoke("update_ytdlp");
  } catch (err) {
    console.error(err); // retry anyway: the current engine may still work
  }
  await invoke("start_download", { id }).catch(console.error);
}

function jobActions(job) {
  const id = job.id;
  const run = (cmd) => () => invoke(cmd, { id }).catch((e) => console.error(e));
  const actions = [];
  switch (job.status) {
    case "downloading":
      // Converting to MP3 takes seconds, and pausing it would only redo the conversion.
      if (!job.converting) actions.push(iconButton(G.pause, "Pause", run("pause_download")));
      actions.push(iconButton(G.stop, "Stop", run("stop_download")));
      break;
    case "paused":
      actions.push(iconButton(G.resume, "Resume", run("start_download")));
      actions.push(iconButton(G.stop, "Stop", run("stop_download")));
      break;
    case "done":
      actions.push(iconButton(G.open, job.format === "mp3" ? "Play MP3" : "Open video", run("open_file")));
      actions.push(iconButton(G.folder, "Show in folder", run("reveal_file")));
      break;
    case "error":
    case "stopped": {
      const fix = job.status === "error" && FIX[job.fix];
      if (fix) {
        const button = el("button", { class: "btn small fix", text: fix.label });
        button.addEventListener("click", () => fix.run(id, button));
        actions.push(button);
      }
      actions.push(iconButton(G.retry, job.status === "stopped" ? "Download again" : "Try again", run("start_download")));
      break;
    }
  }
  if (!IN_PROGRESS.has(job.status)) {
    actions.push(iconButton(G.remove, "Remove from list", () => removeJob(id)));
  }
  return actions;
}

/** Running or paused: can't be removed and survives "Clear". */
const IN_PROGRESS = new Set(["downloading", "paused"]);

const TILE = { downloading: G.download, paused: G.pause, stopped: G.stop, done: G.done, error: G.error };

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
  // Actions are rebuilt only when they change, so hover and focus survive progress updates.
  const actionKey = `${job.status}${job.converting ? ":converting" : ""}${job.fix ? `:${job.fix}` : ""}`;
  const actionsChanged = li.dataset.actions !== actionKey;
  li.className = `job ${job.status}`;
  // A reworded error keeps yt-dlp's original on hover, for searching or reporting it.
  li.title = job.status === "error" && job.errorDetail ? `${job.errorDetail}\n\n${job.url}` : job.url;
  li.querySelector(".job-tile").textContent = TILE[job.status];
  li.querySelector(".job-title").textContent = jobTitle(job);
  li.querySelector(".job-meta").textContent = jobMeta(job);

  let bar = li.querySelector(".progress");
  if (IN_PROGRESS.has(job.status)) {
    if (!bar) {
      bar = el("div", { class: "progress", role: "progressbar", "aria-label": "Download progress" },
        el("div", { class: "progress-bar" }));
      li.querySelector(".job-body").append(bar);
    }
    const known = job.progress > 0;
    bar.classList.toggle("indeterminate", !known && job.status === "downloading");
    bar.firstChild.style.width = `${Math.round(job.progress * 100)}%`;
    if (known) bar.setAttribute("aria-valuenow", Math.round(job.progress * 100));
    else bar.removeAttribute("aria-valuenow");
  } else if (bar) {
    bar.remove();
  }

  if (actionsChanged) {
    const hadFocus = li.querySelector(".job-actions").contains(document.activeElement);
    li.dataset.actions = actionKey;
    li.querySelector(".job-actions").replaceChildren(...jobActions(job));
    // Pause becomes Resume in the same spot, so keyboard users don't lose their place.
    if (hadFocus) li.querySelector(".job-actions button")?.focus();
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
  const finished = list.filter((j) => !IN_PROGRESS.has(j.status)).length;
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
  syncToggle($("notify"), settings.notify);
  $("cookies").value = settings.cookiesBrowser;
  $("quality").value = settings.videoQuality;
  $("parallel").value = String(settings.maxDownloads);
  $("folder-path").textContent = settings.downloadDir;
  $("folder-path").title = settings.downloadDir;
}

async function saveSettings(patch) {
  settings = await invoke("save_settings", { settings: { ...settings, ...patch } });
  renderSettings();
}

const TOOL = {
  installing: { cls: "", title: "Getting ready.", msg: "Downloading the video engine. This only happens once." },
  "installing-converter": { cls: "", title: "Getting ready.", msg: "Downloading the tools for YouTube and MP3s. This only happens once." },
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

// ---------- drag and drop ----------

const carriesText = (dt) => dt.types.includes("text/uri-list") || dt.types.includes("text/plain");

/** A dragged link arrives as text/uri-list (one URL per line, "#" lines are comments); selected text as text/plain. */
function droppedText(dt) {
  const uris = dt.getData("text/uri-list").split(/\r?\n/).filter((l) => l && !l.startsWith("#"));
  return (uris.length ? uris.join("\n") : dt.getData("text/plain")).trim();
}

/** Dropping a link on a box downloads it straight away, like pasting it and pressing Download. */
function initDropTarget(form, addLinks) {
  form.addEventListener("dragover", (e) => {
    if (!carriesText(e.dataTransfer)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
    form.classList.add("drop-target");
  });
  form.addEventListener("dragleave", (e) => {
    if (!form.contains(e.relatedTarget)) form.classList.remove("drop-target");
  });
  form.addEventListener("drop", (e) => {
    e.preventDefault();
    form.classList.remove("drop-target");
    const text = droppedText(e.dataTransfer);
    if (text) addLinks(text);
  });
}

// ---------- trim ----------

/**
 * The scissors button next to a box reveals From/To fields that limit the next download from it
 * to part of the video. They clear and close again once that download is added.
 */
function initClip(form) {
  const toggle = form.querySelector(".trim-toggle");
  const row = form.querySelector(".clip-row");
  const start = row.querySelector('[name="start"]');
  const end = row.querySelector('[name="end"]');
  const setOpen = (open) => {
    row.hidden = !open;
    toggle.setAttribute("aria-expanded", String(open));
    toggle.classList.toggle("on", open);
  };
  toggle.addEventListener("click", () => {
    const open = row.hidden;
    setOpen(open);
    if (open) {
      start.focus();
    } else {
      start.value = end.value = "";
      form.querySelector(".textbox input").focus();
    }
  });
  return {
    range: () => (row.hidden ? {} : { start: start.value, end: end.value }),
    reset: () => {
      start.value = end.value = "";
      setOpen(false);
    },
  };
}

function showAddError(form, msg) {
  const error = form.querySelector(".field-error");
  error.textContent = msg || "";
  error.hidden = !msg;
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

  // Two boxes: the one a link is pasted (or dropped) into decides MP4 (video) or MP3 (audio).
  for (const form of document.querySelectorAll("form.add")) {
    const input = form.querySelector(".textbox input");
    const clip = initClip(form);
    const addLinks = async (text) => {
      try {
        await invoke("add_url", { text, format: form.dataset.format, ...clip.range() });
        input.value = "";
        clip.reset();
        showAddError(form, "");
      } catch (err) {
        showAddError(form, String(err));
      }
    };
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      const text = input.value.trim();
      if (text) addLinks(text);
    });
    form.addEventListener("input", () => showAddError(form, ""));
    initDropTarget(form, addLinks);
  }
  // Anything dropped outside the boxes is ignored, rather than the webview navigating to it.
  for (const type of ["dragover", "drop"]) document.addEventListener(type, (e) => e.preventDefault());

  $("notify").addEventListener("change", (e) => saveSettings({ notify: e.target.checked }));
  $("cookies").addEventListener("change", (e) => saveSettings({ cookiesBrowser: e.target.value }));
  $("quality").addEventListener("change", (e) => saveSettings({ videoQuality: e.target.value }));
  $("parallel").addEventListener("change", (e) => saveSettings({ maxDownloads: Number(e.target.value) }));

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
  showAddError(document.querySelector("form.add"), `ClipDrop couldn't start: ${err}`);
});
