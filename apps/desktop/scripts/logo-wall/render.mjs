// Renders docs/assets/logo-wall.png: every brand OwO supports (apps, provider presets, model
// families) once each, on the theme's dark background, with the OwO mark in the middle.
// The brands come from src/components/vendor-icon.tsx and registry/providers.toml, so the wall
// follows the app. Needs Microsoft Edge (or OWO_SHOTS_BROWSER=chrome) and Python 3 with Pillow.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright-core";

const desktop = join(import.meta.dirname, "..", "..");
const root = join(desktop, "..", "..");
const out = join(root, "docs", "assets", "logo-wall.png");

// ---- brands --------------------------------------------------------------------------------
const source = readFileSync(join(desktop, "src", "components", "vendor-icon.tsx"), "utf8");
const files = Object.fromEntries([...source.matchAll(/import (\w+) from "@lobehub\/icons-static-svg\/icons\/([\w-]+)\.svg\?raw"/g)].map((m) => [m[1], m[2]]));
const block = (name) => source.slice(source.indexOf(`const ${name}`), source.indexOf("};", source.indexOf(`const ${name}`)));
const providers = Object.fromEntries([...block("PROVIDERS").matchAll(/"?([\w-]+)"?: "(\w+)"/g)].map((m) => [m[1], m[2]]));
const apps = [...block("APPS").matchAll(/: "(\w+)"/g)].map((m) => m[1]);
const models = [...block("MODELS").matchAll(/, "(\w+)"\]/g)].map((m) => m[1]);

// Presets whose adapter this build implements (the others are hidden in the app too).
const toml = readFileSync(join(root, "registry", "providers.toml"), "utf8");
const presets = [...toml.matchAll(/^\[presets\.([\w-]+)\]([\s\S]*?)(?=^\[|$(?![\s\S]))/gm)]
  .filter((m) => /adapter = "(openai-chat|anthropic)"/.test(m[2]))
  .map((m) => providers[m[1]])
  .filter(Boolean);

const brands = [...new Set([...apps, ...presets, ...models])].filter((v) => files[v]);
const svg = (v) => readFileSync(join(desktop, "node_modules", "@lobehub", "icons-static-svg", "icons", `${files[v]}.svg`), "utf8").replace(/<title>.*?<\/title>/, "");
const owo = readFileSync(join(desktop, "app-icon.png")).toString("base64");

// ---- layout: a staggered wall, filled from the centre outwards ----------------------------
// The OwO mark is a bit larger than a tile but keeps the same 20px gap to its neighbours.
const W = 1600, H = 640, TILE = 104, PITCH = 124, ROWS = 5, OWO = 124;
const cx = W / 2, cy = H / 2;
const cells = [];
for (let r = 0; r < ROWS; r++) {
  const y = cy + (r - (ROWS - 1) / 2) * PITCH;
  const shift = r % 2 ? PITCH / 2 : 0;
  for (let x = cx + shift - Math.ceil(W / PITCH) * PITCH; x < W + PITCH; x += PITCH) cells.push({ x, y });
}
const dist = (c) => Math.hypot((c.x - cx) / (W / 2), (c.y - cy) / (H / 2));
const far = Math.max(...cells.map(dist));
// Logos fill a symmetric ellipse: this many per row, centred (even rows have a middle cell, the
// staggered ones don't). The rest of the wall is blank tiles that fade into the background.
const QUOTA = [5, 10, 11, 10, 5];
const rowOf = (c) => Math.round((c.y - cy) / PITCH + (ROWS - 1) / 2);
const inner = QUOTA.flatMap((n, r) => cells.filter((c) => rowOf(c) === r).sort((a, b) => Math.abs(a.x - cx) - Math.abs(b.x - cx)).slice(0, n));
if (inner.length !== brands.length + 1) console.warn(`the ellipse holds ${inner.length - 1} logos for ${brands.length} brands; adjust QUOTA`);
// Brands in priority order (apps, providers, models) go from the centre outwards.
inner.sort((a, b) => dist(a) - dist(b));
const blanks = cells.filter((c) => !inner.includes(c));

const tiles = [...inner, ...blanks].map((c, i) => {
  const t = Math.min(1, dist(c) / far);
  const fade = (1 - 0.8 * t * t).toFixed(3);
  const pos = `left:${c.x - TILE / 2}px;top:${c.y - TILE / 2}px`;
  if (i === 0) return `<div class="owo" style="left:${c.x - OWO / 2}px;top:${c.y - OWO / 2}px"><img src="data:image/png;base64,${owo}" alt=""></div>`;
  const brand = i <= brands.length && i < inner.length ? brands[i - 1] : undefined;
  if (!brand) return `<div class="tile blank" style="${pos};opacity:${(fade * 0.55).toFixed(3)}"></div>`;
  const mono = /fill="currentColor"/.test(svg(brand)) ? " mono" : "";
  return `<div class="tile${mono}" style="${pos};opacity:${fade}">${svg(brand)}</div>`;
});

const html = `<!doctype html><html><head><meta charset="utf-8"><style>
  html, body { margin: 0; background: #0a0a0a; }
  .wall { position: relative; width: ${W}px; height: ${H}px; overflow: hidden; background: #0a0a0a; }
  .tile { position: absolute; width: ${TILE}px; height: ${TILE}px; box-sizing: border-box; border-radius: 28px;
    background: #171717; border: 1px solid rgba(255,255,255,.06); box-shadow: inset 0 1px 0 rgba(255,255,255,.04);
    display: flex; align-items: center; justify-content: center; }
  .tile svg { width: 50px; height: 50px; display: block; }
  .tile.mono { color: #fafafa; }
  .owo { position: absolute; width: ${OWO}px; height: ${OWO}px; }
  .owo::before { content: ""; position: absolute; inset: -60px; border-radius: 50%;
    background: radial-gradient(closest-side, rgba(120,190,255,.22), rgba(120,255,200,.08) 60%, transparent); }
  .owo img { position: relative; width: 100%; height: 100%; display: block; }
  .veil { position: absolute; inset: 0; pointer-events: none;
    background: radial-gradient(ellipse 62% 78% at 50% 50%, transparent 55%, rgba(10,10,10,.85) 100%); }
</style></head><body><div class="wall">${tiles.join("")}<div class="veil"></div></div></body></html>`;

// ---- capture -----------------------------------------------------------------------------
const browser = await chromium.launch({ channel: process.env.OWO_SHOTS_BROWSER || "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: W, height: H }, deviceScaleFactor: 2, colorScheme: "dark" });
await page.setContent(html, { waitUntil: "load" });
await page.locator(".wall").screenshot({ path: out });
await browser.close();

// Lossless: a 256-colour palette bands the fading edges.
execFileSync("python", ["-c", "import sys; from PIL import Image; p = sys.argv[1]; Image.open(p).convert('RGB').save(p, optimize=True)", out], { stdio: "inherit" });
console.log(`${brands.length} brands → ${out}`);
console.log(brands.join(", "));
