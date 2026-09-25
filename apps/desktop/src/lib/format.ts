import i18next from "i18next";

/** `812`, `95.3K`, `1.23M` — the same shapes the CLI prints. */
export function compact(n: number): string {
  if (n < 1_000) return String(n);
  if (n < 1_000_000) return `${trim((n / 1e3).toFixed(1))}K`;
  if (n < 1_000_000_000) return `${trim((n / 1e6).toFixed(2))}M`;
  return `${trim((n / 1e9).toFixed(2))}B`;
}

function trim(s: string): string {
  return s.includes(".") ? s.replace(/0+$/, "").replace(/\.$/, "") : s;
}

/** `1,234,567`. */
export function exact(n: number): string {
  return n.toLocaleString("en-US");
}

/** `$0.0042` below a cent, else `$1.23`. */
export function money(usd: number): string {
  if (usd === 0) return "$0";
  return usd < 0.01 ? `$${usd.toFixed(4)}` : `$${usd.toFixed(2)}`;
}

/** `850ms`, `4.2s`, `2m05s`. */
export function duration(ms: number): string {
  if (ms < 1_000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1e3).toFixed(1)}s`;
  return `${Math.floor(ms / 60_000)}m${String(Math.floor((ms % 60_000) / 1000)).padStart(2, "0")}s`;
}

const APP_NAMES: Record<string, string> = {
  codex: "codex",
  codex_desktop: "codex-desktop",
  claude_code: "claude",
  claude_desktop: "claude-desktop",
  cursor: "cursor",
  grok_build: "grok",
  opencode: "opencode",
  minimax_code: "mcode",
  minimax_cli: "mmx",
  zcode: "zcode",
  copilot_app: "copilot",
};

/** The `owo connect` name for a client integration id. */
export function appName(client: string | null | undefined): string {
  if (!client) return "—";
  return APP_NAMES[client] ?? client;
}

const APP_TITLES: Record<string, string> = {
  codex: "Codex",
  "codex-desktop": "Codex Desktop",
  claude: "Claude Code",
  "claude-desktop": "Claude Desktop",
  cursor: "Cursor",
  grok: "Grok Build",
  opencode: "OpenCode",
  mcode: "MiniMax Code",
  mmx: "MiniMax CLI",
  zcode: "ZCode",
  copilot: "GitHub Copilot",
};

/** Human-readable product name for an `owo connect` app id (`claude` → `Claude Code`). */
export function appTitle(app: string): string {
  if (!app || app === "—") return app;
  return (
    APP_TITLES[app] ??
    app
      .split("-")
      .map((w) => (w ? w[0].toUpperCase() + w.slice(1) : w))
      .join(" ")
  );
}

/** Translation keys under `apps.kind` for the kind-of-app line on the Apps card. */
const APP_KINDS: Record<string, string> = {
  codex: "cliProfile",
  "codex-desktop": "desktop",
  claude: "cliIde",
  "claude-desktop": "desktop",
  cursor: "ide",
  grok: "cli",
  opencode: "cli",
  mcode: "cli",
  zcode: "ide",
  copilot: "desktop",
};

/** A short, fixed kind-of-app line for the Apps card (`claude` → `CLI & IDE`); empty when unknown. */
export function appSubtitle(app: string): string {
  const kind = APP_KINDS[app];
  return kind ? i18next.t(`apps.kind.${kind}`) : "";
}

/** `YYYY-MM-DD` (a local-date key) shown as a short day label in the UI language, e.g. `9/24` or `9月24日`. */
export function shortDate(key: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(key);
  if (!m) return key;
  const date = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  return date.toLocaleDateString(i18next.language, { month: "numeric", day: "numeric" });
}

/** A month abbreviation in the UI language (`Sep`, `9月`). */
export function monthLabel(date: Date): string {
  return date.toLocaleDateString(i18next.language, { month: "short" });
}

export function priceLabel(price: { input: number; output: number } | null | undefined): string {
  if (!price) return "—";
  return `$${trim(price.input.toFixed(4))} / $${trim(price.output.toFixed(4))}`;
}
