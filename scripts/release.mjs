#!/usr/bin/env node
// Builds a signed ClipDrop release and the `latest.json` feed the in-app updater reads.
//
//   npm run release -- --notes "What changed"            build only, then upload by hand
//   npm run release -- --notes "What changed" --publish  also create the GitHub release (needs `gh`)
//
// Before releasing, bump "version" in src-tauri/tauri.conf.json (and Cargo.toml / package.json).
//
// Env:
//   TAURI_SIGNING_PRIVATE_KEY_PATH      default: ~/.tauri/clipdrop.key
//   TAURI_SIGNING_PRIVATE_KEY_PASSWORD  prompted for (hidden) if not set

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const conf = JSON.parse(fs.readFileSync(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const version = conf.version;
const product = conf.productName;

const args = process.argv.slice(2);
const flag = (name) => args.includes(name);
const option = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};
const notes = option("--notes") ?? "";
const publish = flag("--publish");

const fail = (msg) => {
  console.error(`\nrelease: ${msg}`);
  process.exit(1);
};

// The updater endpoint names the GitHub repo that hosts releases.
const endpoint = conf.plugins?.updater?.endpoints?.[0] ?? "";
const repo = endpoint.match(/github\.com\/([^/]+\/[^/]+)\/releases/)?.[1];
if (!repo) fail(`can't find a GitHub repo in the updater endpoint: ${endpoint}`);
const tag = `v${version}`;

const keyPath = process.env.TAURI_SIGNING_PRIVATE_KEY_PATH ?? path.join(os.homedir(), ".tauri", "clipdrop.key");
if (!fs.existsSync(keyPath)) fail(`signing key not found at ${keyPath}`);

function promptHidden(question) {
  return new Promise((resolve) => {
    const stdin = process.stdin;
    if (!stdin.isTTY) fail("set TAURI_SIGNING_PRIVATE_KEY_PASSWORD (no terminal to prompt in)");
    process.stdout.write(question);
    stdin.setRawMode(true);
    stdin.resume();
    stdin.setEncoding("utf8");
    let value = "";
    const onData = (ch) => {
      if (ch === "\r" || ch === "\n" || ch === "\u0004") {
        stdin.setRawMode(false);
        stdin.pause();
        stdin.off("data", onData);
        process.stdout.write("\n");
        resolve(value);
      } else if (ch === "\u0003") {
        process.exit(130);
      } else if (ch === "\u007f" || ch === "\b") {
        value = value.slice(0, -1);
      } else {
        value += ch;
      }
    };
    stdin.on("data", onData);
  });
}

const password =
  process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? (await promptHidden("Update-signing key password: "));

console.log(`Building ${product} ${version} (signed updater artifacts)...`);
// A config file rather than inline JSON, which the Windows shell would mangle.
const extraConf = path.join(os.tmpdir(), "clipdrop-release.conf.json");
fs.writeFileSync(extraConf, JSON.stringify({ bundle: { createUpdaterArtifacts: true } }));
const build = spawnSync(
  `npx tauri build --config "${extraConf}"`,
  {
    cwd: root,
    stdio: "inherit",
    shell: true,
    env: {
      ...process.env,
      TAURI_SIGNING_PRIVATE_KEY: fs.readFileSync(keyPath, "utf8"),
      TAURI_SIGNING_PRIVATE_KEY_PASSWORD: password,
      // Keep the builder's home folder (and so their Windows username) out of the binary.
      RUSTFLAGS: [process.env.RUSTFLAGS, `--remap-path-prefix=${os.homedir()}=~`].filter(Boolean).join(" "),
    },
  },
);
if (build.status !== 0) fail("build failed");

const bundleDir = path.join(root, "src-tauri", "target", "release", "bundle", "nsis");
const installer = `${product}_${version}_x64-setup.exe`;
const installerPath = path.join(bundleDir, installer);
const sigPath = `${installerPath}.sig`;
if (!fs.existsSync(installerPath) || !fs.existsSync(sigPath)) fail(`missing ${installer} or its .sig in ${bundleDir}`);

const feed = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature: fs.readFileSync(sigPath, "utf8").trim(),
      url: `https://github.com/${repo}/releases/download/${tag}/${installer}`,
    },
  },
};
const feedPath = path.join(bundleDir, "latest.json");
fs.writeFileSync(feedPath, JSON.stringify(feed, null, 2) + "\n");

console.log(`\nBuilt:\n  ${installerPath}\n  ${feedPath}`);

if (publish) {
  const gh = spawnSync(
    "gh",
    ["release", "create", tag, installerPath, feedPath, "--repo", repo, "--title", `${product} ${version}`, "--notes", notes || `${product} ${version}`],
    { stdio: "inherit" },
  );
  if (gh.status !== 0) fail("gh release create failed (is `gh` installed and logged in?)");
  console.log(`\nPublished ${tag}. Installed copies will offer the update on their next check.`);
} else {
  console.log(`
To publish, create a GitHub release at https://github.com/${repo}/releases/new
  tag: ${tag}
  attach: ${installer} and latest.json (both from the folder above)
Installed copies will offer the update on their next check.`);
}
