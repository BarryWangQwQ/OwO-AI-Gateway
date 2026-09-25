// Builds the single-file release: the desktop app with the `owo` CLI inside, no installer.
// Output: target/release/OwO-AI-Gateway_<version>_<arch>.exe (or without .exe elsewhere).
import { execSync } from "node:child_process";
import { copyFileSync, readFileSync } from "node:fs";
import { join } from "node:path";

const desktop = join(import.meta.dirname, "..");
const root = join(desktop, "..", "..");
const { version } = JSON.parse(readFileSync(join(desktop, "src-tauri", "tauri.conf.json"), "utf8"));

execSync("npx tauri build --no-bundle", { cwd: desktop, stdio: "inherit" });

const ext = process.platform === "win32" ? ".exe" : "";
const arch = { x64: "x64", arm64: "arm64" }[process.arch] ?? process.arch;
const out = join(root, "target", "release", `OwO-AI-Gateway_${version}_${arch}${ext}`);
copyFileSync(join(root, "target", "release", `owo-desktop${ext}`), out);
console.log(`\nsingle-file build: ${out}`);
