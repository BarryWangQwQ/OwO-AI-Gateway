// Builds the release for the current platform, with the `owo` CLI inside the app:
//   Windows  a single executable, no installer          OwO-AI-Gateway_<version>_windows-<arch>.exe
//   macOS    OwO AI Gateway.app inside a disk image      OwO-AI-Gateway_<version>_macos-<arch>.dmg
//   Linux    an AppImage carrying its own libraries      OwO-AI-Gateway_<version>_linux-<arch>.AppImage
// The files land in target/release/.
import { execSync } from "node:child_process";
import { copyFileSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const desktop = join(import.meta.dirname, "..");
const release = join(desktop, "..", "..", "target", "release");
const { version } = JSON.parse(readFileSync(join(desktop, "src-tauri", "tauri.conf.json"), "utf8"));
const name = (os, ext) => join(release, `OwO-AI-Gateway_${version}_${os}-${process.arch}${ext}`);
// Bundling is off in tauri.conf.json so a plain `tauri build` stays a bare executable.
const bundle = (targets) => `npx tauri build --bundles ${targets} --config "{\\"bundle\\":{\\"active\\":true}}"`;
const only = (dir, ext) => {
  const found = readdirSync(dir).filter((f) => f.endsWith(ext));
  if (found.length !== 1) throw new Error(`expected one ${ext} in ${dir}, found ${found.length}`);
  return join(dir, found[0]);
};

let out;
if (process.platform === "win32") {
  execSync("npx tauri build --no-bundle", { cwd: desktop, stdio: "inherit" });
  out = name("windows", ".exe");
  copyFileSync(join(release, "owo-desktop.exe"), out);
} else if (process.platform === "darwin") {
  execSync(bundle("app,dmg"), { cwd: desktop, stdio: "inherit" });
  out = name("macos", ".dmg");
  copyFileSync(only(join(release, "bundle", "dmg"), ".dmg"), out);
} else {
  execSync(bundle("appimage"), { cwd: desktop, stdio: "inherit" });
  out = name("linux", ".AppImage");
  copyFileSync(only(join(release, "bundle", "appimage"), ".AppImage"), out);
}
console.log(`\nrelease: ${out}`);
