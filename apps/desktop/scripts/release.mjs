// Builds the single-file release: the desktop app with the `owo` CLI inside, no installer.
// Output: target/release/OwO-AI-Gateway_<version>_<os>-<arch>[.exe], e.g. _windows-x64.exe, _macos-arm64.
import { execSync } from "node:child_process";
import { copyFileSync, readFileSync } from "node:fs";
import { join } from "node:path";

const desktop = join(import.meta.dirname, "..");
const root = join(desktop, "..", "..");
const { version } = JSON.parse(readFileSync(join(desktop, "src-tauri", "tauri.conf.json"), "utf8"));

execSync("npx tauri build --no-bundle", { cwd: desktop, stdio: "inherit" });

const ext = process.platform === "win32" ? ".exe" : "";
const os = { win32: "windows", darwin: "macos", linux: "linux" }[process.platform] ?? process.platform;
const out = join(root, "target", "release", `OwO-AI-Gateway_${version}_${os}-${process.arch}${ext}`);
copyFileSync(join(root, "target", "release", `owo-desktop${ext}`), out);
console.log(`\nsingle-file build: ${out}`);
