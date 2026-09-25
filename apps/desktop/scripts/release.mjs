// Builds the release for the current platform, with the `owo` CLI inside the app:
//   Windows  a single executable, no installer          OwO-AI-Gateway_<version>_windows-<arch>.exe
//   macOS    OwO AI Gateway.app inside a disk image      OwO-AI-Gateway_<version>_macos-<arch>.dmg
//   Linux    an AppImage carrying its own libraries      OwO-AI-Gateway_<version>_linux-<arch>.AppImage
// The files land in target/release/.
import { execFileSync, execSync } from "node:child_process";
import { copyFileSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const desktop = join(import.meta.dirname, "..");
const tauriDir = join(desktop, "src-tauri");
const release = join(desktop, "..", "..", "target", "release");
const { version } = JSON.parse(readFileSync(join(tauriDir, "tauri.conf.json"), "utf8"));
const name = (os, ext) => join(release, `OwO-AI-Gateway_${version}_${os}-${process.arch}${ext}`);
// Bundling is off in tauri.conf.json so a plain `tauri build` stays a bare executable.
const bundle = (targets, extra = {}) =>
  execFileSync("npx", ["tauri", "build", "--bundles", targets, "--config", JSON.stringify({ bundle: { active: true, ...extra } })], {
    cwd: desktop,
    stdio: "inherit",
  });

/**
 * macOS 26 shows an app's icon from a compiled Assets.car and puts icons that come only as .icns on a grey
 * plate. Compiles icons/AppIcon.icon (Icon Composer format) with Xcode's actool (Xcode 26 or newer) and
 * returns the bundle resource mapping; Info.plist names the icon `AppIcon`, icon.icns stays for older macOS.
 */
function macIconResources() {
  const outDir = join(tauriDir, "icons", "macos-build");
  mkdirSync(outDir, { recursive: true });
  execFileSync(
    "xcrun",
    ["actool", join(tauriDir, "icons", "AppIcon.icon"), "--compile", outDir, "--platform", "macosx", "--minimum-deployment-target", "11.0",
      "--app-icon", "AppIcon", "--include-all-app-icons", "--output-partial-info-plist", join(outDir, "partial.plist")],
    { stdio: "inherit" },
  );
  return { resources: { "icons/macos-build/Assets.car": "Assets.car" } };
}
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
  bundle("app,dmg", macIconResources());
  out = name("macos", ".dmg");
  copyFileSync(only(join(release, "bundle", "dmg"), ".dmg"), out);
} else {
  bundle("appimage");
  out = name("linux", ".AppImage");
  copyFileSync(only(join(release, "bundle", "appimage"), ".AppImage"), out);
}
console.log(`\nrelease: ${out}`);
