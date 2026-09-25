// Captures the README screenshots: serves the app against ./mock-tauri.ts (port 1430) and drives
// it with the installed Microsoft Edge through playwright-core, then shrinks the PNGs (optimize.py).
//
//   npm run screenshots                 # zh-CN and en → docs/screenshots/{zh,en}/
//   npm run screenshots -- en           # one language
//   npm run screenshots -- zh-CN mcp    # one language, only shots whose name contains "mcp"
//
// Env: OWO_SHOTS_BROWSER=chrome to use Google Chrome instead of Edge; OWO_SHOTS_NO_OPTIMIZE=1 to keep raw PNGs.
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright-core";
import { createServer } from "vite";

const here = path.dirname(fileURLToPath(import.meta.url));
const app = path.resolve(here, "../..");
const repo = path.resolve(app, "../..");
const outRoot = path.join(repo, "docs", "screenshots");

const LANGS = { "zh-CN": "zh", en: "en" };
const args = process.argv.slice(2);
const langs = args.filter((a) => a in LANGS);
const only = args.filter((a) => !(a in LANGS));

// One fixed moment (a Thursday afternoon, Beijing time) so the charts and dates are the same on every run.
const NOW = new Date("2026-09-24T16:20:00+08:00");
const TIMEZONE = "Asia/Shanghai";
const VIEWPORT = { width: 1240, height: 800 };

function strings(lang) {
  const json = JSON.parse(readFileSync(path.join(app, "src", "i18n", "locales", `${lang}.json`), "utf8"));
  return (key) => {
    const value = key.split(".").reduce((o, k) => o?.[k], json);
    if (typeof value !== "string") throw new Error(`no ${lang} string for ${key}`);
    return value;
  };
}

/** Waits until no skeleton or busy placeholder is left and the charts have drawn. */
async function settle(page, extra = 900) {
  await page.waitForFunction(() => !document.querySelector('[data-slot="skeleton"], [aria-busy="true"]'), null, { timeout: 15_000 });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(extra);
  await page.mouse.move(4, 4);
  await page.waitForTimeout(150);
}

const nav = async (page, t, key) => {
  await page.locator('[data-sidebar="menu-button"]', { hasText: t(`nav.${key}`) }).click();
  await settle(page);
};

const button = (page, label) => page.locator(`button[aria-label="${label}"]`);

/** Focus left on a dialog button would open its tooltip; drop it. */
const blur = (page) => page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());

/** Shot name → how to get there from a fresh start (the dashboard). */
const SHOTS = {
  dashboard: async (page) => settle(page, 1_400),
  // 30 days by app: app names fit under the bars, where long model names would overlap.
  usage: async (page, t) => {
    await nav(page, t, "usage");
    await page.locator('[data-slot="toggle-group-item"]').nth(2).click();
    await page.locator('[data-slot="select-trigger"]').first().click();
    await page.getByRole("option", { name: t("usage.by.app") }).click();
    await settle(page, 1_400);
  },
  history: async (page, t) => nav(page, t, "history"),
  "history-detail": async (page, t) => {
    await nav(page, t, "history");
    await page.locator('[data-slot="table-body"] [data-slot="table-row"]').nth(1).click();
    await page.locator('[data-slot="sheet-content"]').waitFor();
    await blur(page);
    await settle(page);
  },
  providers: async (page, t) => nav(page, t, "providers"),
  "provider-wizard": async (page, t) => {
    await nav(page, t, "providers");
    await button(page, t("providers.add")).click();
    await page.locator('[data-slot="dialog-content"]').waitFor();
    await blur(page);
    await settle(page);
  },
  models: async (page, t) => nav(page, t, "models"),
  apps: async (page, t) => nav(page, t, "apps"),
  mcp: async (page, t) => nav(page, t, "mcp"),
  "mcp-edit": async (page, t) => {
    await nav(page, t, "mcp");
    await button(page, t("mcp.actions")).nth(1).click();
    await page.getByRole("menuitem", { name: t("common.edit") }).click();
    await page.locator('[data-slot="dialog-content"]').waitFor();
    await blur(page);
    await settle(page);
  },
  skills: async (page, t) => nav(page, t, "skills"),
  "skills-discover": async (page, t) => {
    await nav(page, t, "skills");
    await button(page, t("skills.discover")).click();
    await page.locator('[data-slot="dialog-content"]').waitFor();
    await blur(page);
    await settle(page);
  },
  "skills-editor": async (page, t) => {
    await nav(page, t, "skills");
    await button(page, t("skills.actions")).first().click();
    await page.getByRole("menuitem", { name: t("skills.edit") }).click();
    await page.locator('[data-slot="dialog-content"] .cm-editor').waitFor();
    await blur(page);
    await settle(page);
  },
  settings: async (page, t) => {
    await nav(page, t, "settings");
    await page.locator(".cm-editor").waitFor();
    await settle(page);
  },
  "settings-config": async (page, t) => {
    await nav(page, t, "settings");
    await page.locator(".cm-editor").waitFor();
    await page.evaluate(() => document.querySelector(".cm-editor")?.closest('[data-slot="card"]')?.scrollIntoView({ block: "start" }));
    await settle(page);
  },
};

const server = await createServer({ configFile: path.join(here, "vite.config.ts"), logLevel: "warn" });
await server.listen();
const base = `http://localhost:${server.config.server.port}/scripts/screenshots/index.html`;
const browser = await chromium.launch({ channel: process.env.OWO_SHOTS_BROWSER || "msedge", headless: true });

const written = [];
try {
  for (const lang of langs.length ? langs : Object.keys(LANGS)) {
    const t = strings(lang);
    const dir = path.join(outRoot, LANGS[lang]);
    mkdirSync(dir, { recursive: true });
    for (const [name, go] of Object.entries(SHOTS)) {
      if (only.length && !only.some((o) => name.includes(o))) continue;
      const context = await browser.newContext({ viewport: VIEWPORT, deviceScaleFactor: 2, colorScheme: "dark", locale: lang, timezoneId: TIMEZONE });
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", (e) => errors.push(e.message));
      await page.clock.setFixedTime(NOW);
      await page.goto(`${base}?lang=${lang}`);
      await page.locator('[data-sidebar="menu-button"]').first().waitFor();
      await go(page, t);
      if (errors.length) throw new Error(`${lang}/${name}: ${errors.join("; ")}`);
      const file = path.join(dir, `${name}.png`);
      await page.screenshot({ path: file });
      written.push(file);
      console.log(`captured ${path.relative(repo, file)}`);
      await context.close();
    }
  }
} finally {
  await browser.close();
  await server.close();
}

if (!process.env.OWO_SHOTS_NO_OPTIMIZE && written.length) {
  const python = process.platform === "win32" ? "python" : "python3";
  const run = spawnSync(python, [path.join(here, "optimize.py"), ...written], { stdio: "inherit" });
  if (run.error || run.status !== 0) console.warn("optimize.py did not run (needs Python 3 with Pillow); the PNGs are left as captured.");
}
