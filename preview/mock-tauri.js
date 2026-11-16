// Fake Tauri APIs so the real UI (../src) can be opened directly in a browser.
// The state shown is picked by data-mode on this script tag: list | empty | settings | installing | error.
(() => {
  const MODE = document.currentScript.dataset.mode || "list";
  const jobs = MODE === "empty" || MODE === "installing" || MODE === "error" ? [] : [
    { id: 5, url: "https://x.com/i/status/1857000000000000001", platform: "twitter", format: "mp4", status: "downloading", title: "orbitwatch - Booster catch, full tower view", progress: 0.62, speed: 4300000 },
    { id: 7, url: "https://www.youtube.com/watch?v=aqz-KE-bpKQ", platform: "youtube", format: "mp4", status: "paused", title: "Big Buck Bunny 60fps 4K", progress: 0.37, speed: null },
    { id: 6, url: "https://x.com/i/status/1857000000000000002", platform: "twitter", format: "mp3", status: "stopped", title: "launch day recap", progress: 0, speed: null },
    { id: 4, url: "https://vm.tiktok.com/ZMabc123/", platform: "tiktok", format: "mp4", status: "downloading", title: null, progress: 0, speed: null },
    { id: 3, url: "https://www.tiktok.com/@kitchen.lab/video/7431", platform: "tiktok", format: "mp4", status: "done", title: "30-second ramen upgrade #cooking", progress: 1, file: String.raw`C:\Users\me\Downloads\ClipDrop\tiktok_kitchen.lab_7431000000.mp4` },
    { id: 2, url: "https://www.instagram.com/reel/C9xQ/", platform: "instagram", format: "mp4", status: "error", title: null, progress: 0, error: "Instagram requires you to be logged in. Choose a browser under Settings > Use browser sign-in." },
    { id: 1, url: "https://www.youtube.com/watch?v=jNQXAC9IVRw", platform: "youtube", format: "mp3", status: "done", title: "Me at the zoo", progress: 1, file: String.raw`C:\Users\me\Downloads\ClipDrop\Me at the zoo [jNQXAC9IVRw].mp3` },
  ];
  const toolStatus = MODE === "installing" ? "installing" : MODE === "error" ? "error" : "ready";
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        console.log("[mock invoke]", cmd, args ?? "");
        if (cmd === "get_snapshot") {
          return {
            settings: { downloadDir: String.raw`C:\Users\me\Downloads\ClipDrop`, cookiesBrowser: "", notify: true },
            jobs, toolStatus, mica: false,
          };
        }
        if (cmd === "save_settings") return args.settings;
        if (cmd === "clear_finished") return jobs.filter((j) => j.status === "downloading" || j.status === "paused");
        return null;
      },
    },
    event: { listen: async () => () => {} },
    window: {
      getCurrentWindow: () => ({
        minimize() {}, toggleMaximize() {}, close() {}, onResized() {},
        isMaximized: async () => false,
      }),
    },
  };
  if (MODE === "settings") {
    addEventListener("load", () => setTimeout(() => document.getElementById("open-settings").click(), 50));
  }
})();
