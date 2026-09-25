// Stand-in for `@tauri-apps/api/core` and `@tauri-apps/api/window` in the screenshot build
// (aliased in ./vite.config.ts): every command the pages invoke, answered from ./mock-data.ts.
import type { CallQuery, GroupBy, StoredCall, Summary, UsageReport } from "@/lib/api";

import * as data from "./mock-data";

const empty = (key: string): Summary => ({
  key,
  calls: 0,
  failed: 0,
  cancelled: 0,
  input_tokens: 0,
  cached_input_tokens: 0,
  cache_creation_input_tokens: 0,
  output_tokens: 0,
  reasoning_tokens: 0,
  cost_usd: null,
  unpriced: 0,
});

function add(s: Summary, c: StoredCall) {
  s.calls++;
  if (c.status === "error") s.failed++;
  if (c.status === "cancelled") s.cancelled++;
  s.input_tokens += c.input_tokens ?? 0;
  s.cached_input_tokens += c.cached_input_tokens ?? 0;
  s.cache_creation_input_tokens += c.cache_creation_input_tokens ?? 0;
  s.output_tokens += c.output_tokens ?? 0;
  s.reasoning_tokens += c.reasoning_tokens ?? 0;
  if (c.cost_usd != null) s.cost_usd = (s.cost_usd ?? 0) + c.cost_usd;
  else if ((c.input_tokens ?? 0) + (c.output_tokens ?? 0) > 0) s.unpriced++;
}

/** The first local day of a `days`-day period ending today. */
function since(days: number): string {
  const d = new Date(data.now);
  d.setHours(0, 0, 0, 0);
  d.setDate(d.getDate() - (Math.max(1, days) - 1));
  return data.dayKey(d);
}

const inPeriod = (days: number) => {
  const start = since(days);
  return data.calls.filter((c) => c.time.slice(0, 10) >= start);
};

const keyOf: Record<GroupBy, (c: StoredCall) => string> = {
  model: (c) => c.model ?? c.requested_model,
  app: (c) => c.client ?? "-",
  provider: (c) => c.provider ?? "-",
  day: (c) => c.time.slice(0, 10),
};

function usage(days: number, by: GroupBy): UsageReport {
  const groups = new Map<string, Summary>();
  const total = empty("TOTAL");
  for (const c of inPeriod(days)) {
    const key = keyOf[by](c);
    if (!groups.has(key)) groups.set(key, empty(key));
    add(groups.get(key)!, c);
    add(total, c);
  }
  const rows = [...groups.values()];
  if (by === "day") rows.sort((a, b) => a.key.localeCompare(b.key));
  else rows.sort((a, b) => b.input_tokens + b.output_tokens - (a.input_tokens + a.output_tokens) || b.calls - a.calls);
  return { since: since(days), rows, total };
}

function today(): Summary {
  const s = empty("");
  inPeriod(1).forEach((c) => add(s, c));
  return s;
}

function calls(q: CallQuery): StoredCall[] {
  const out: StoredCall[] = [];
  for (let i = data.calls.length - 1; i >= 0 && out.length < q.limit; i--) {
    const c = data.calls[i];
    if (q.failedOnly && c.status !== "error") continue;
    if (q.model && c.model !== q.model && c.requested_model !== q.model) continue;
    if (q.client && c.client !== q.client) continue;
    out.push(c);
  }
  return out;
}

const done = (output = "") => ({ ok: true, output });

type Args = Record<string, any>;

const HANDLERS: Record<string, (a: Args) => unknown> = {
  status: () => data.status,
  usage: (a) => usage(a.days, a.by),
  today,
  calls: (a) => calls(a.query),
  call: (a) => data.calls.find((c) => c.id === a.id) ?? null,
  clear_history: () => 0,
  models: () => data.models,
  providers: () => data.providers,
  presets: () => data.presets,
  apps: () => data.apps,
  general: () => data.general,
  config_text: () => ({ path: data.status.configPath, text: data.configText }),
  check_config_text: () => null,
  mcp_list: () => ({ apps: data.mcpApps, servers: data.mcpServers }),
  mcp_scan: () => data.mcpFound,
  skills_list: () => ({ store: data.skillStore, apps: data.skillApps, skills: data.skills, unmanaged: data.unmanagedSkills }),
  skills_discovered: () => data.skillRepos,
  skills_discover: () => data.skillRepos,
  skills_repos: () => data.skillRepos.map((r) => r.repo),
  skills_tree: (a) => data.skillTrees[a.name] ?? { path: `${data.skillStore}/${a.name}`, entries: [{ path: "SKILL.md", depth: 0, dir: false, size: 900, link: false }], more: 0 },
  skills_read: (a) => data.skillFile(a.name, a.path),
  skills_pick: () => null,
  set_key: (a) => `keyring:${a.name}`,
};

export async function invoke<T>(cmd: string, args: Args = {}): Promise<T> {
  await new Promise((resolve) => setTimeout(resolve, 15));
  const handler = HANDLERS[cmd];
  // Everything else changes state; the screenshots never get that far, so it just succeeds.
  const result = handler ? handler(args) : done();
  return structuredClone(result) as T;
}

export function getCurrentWindow() {
  return { setTheme: async (_theme: string | null) => {} };
}
