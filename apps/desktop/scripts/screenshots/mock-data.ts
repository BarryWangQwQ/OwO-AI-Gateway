// Fictional sample data for the README screenshots. Nothing here comes from a real machine:
// paths live under /home/you, keys are references only, hosts are public vendor endpoints or example.com.
import type { AppInfo, General, McpAppInfo, McpAppState, McpFound, McpKeyValue, McpServer, ModelInfo, Preset, Price, ProviderInfo, Status, StoredCall } from "@/lib/api";
import type { Skill, SkillApp, SkillAppId, SkillAppState, SkillFile, SkillRepo, SkillTree } from "@/lib/skills-api";

const HOME = "/home/you";

/** Text the user wrote themselves (descriptions of their own servers and skills) follows the screenshot language. */
const zh = localStorage.getItem("owo-lang") === "zh-CN";
const tr = (en: string, zhText: string) => (zh ? zhText : en);

// ---------------------------------------------------------------- deterministic randomness

function mulberry32(seed: number) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const rng = mulberry32(20260925);
const between = (lo: number, hi: number) => lo + rng() * (hi - lo);
const int = (lo: number, hi: number) => Math.round(between(lo, hi));
/** Log-normal-ish around `median`, clamped to [lo, hi]. */
function skewed(median: number, spread: number, lo: number, hi: number): number {
  const g = (rng() + rng() + rng() - 1.5) * 2;
  return Math.round(Math.min(hi, Math.max(lo, median * Math.exp(g * spread))));
}
function pick<T extends string>(weights: Partial<Record<T, number>>): T {
  const entries = Object.entries(weights) as [T, number][];
  let r = rng() * entries.reduce((s, [, w]) => s + w, 0);
  for (const [k, w] of entries) {
    r -= w;
    if (r <= 0) return k;
  }
  return entries[entries.length - 1][0];
}
const hex = (n: number) => Array.from({ length: n }, () => Math.floor(rng() * 16).toString(16)).join("");

// ---------------------------------------------------------------- status, presets

export const status: Status = {
  version: "0.1.0",
  configPath: `${HOME}/.owo/config.toml`,
  configExists: true,
  configError: null,
  configBackupsPath: `${HOME}/.owo/backups/config`,
  name: "OwO",
  gatewayRunning: true,
  gatewayAddress: "127.0.0.1:8787",
  owoBinary: `${HOME}/.cargo/bin/owo`,
};

/** `registry/providers.toml`, minus presets whose adapter the desktop app hides (`google`). */
export const presets: Preset[] = (
  [
    ["openai", "OpenAI", "https://api.openai.com/v1", "OPENAI_API_KEY"],
    ["anthropic", "Anthropic", "https://api.anthropic.com", "ANTHROPIC_API_KEY", "anthropic"],
    ["openrouter", "OpenRouter", "https://openrouter.ai/api/v1", "OPENROUTER_API_KEY"],
    ["deepseek", "DeepSeek", "https://api.deepseek.com", "DEEPSEEK_API_KEY"],
    ["xai", "xAI", "https://api.x.ai/v1", "XAI_API_KEY"],
    ["groq", "Groq", "https://api.groq.com/openai/v1", "GROQ_API_KEY"],
    ["cerebras", "Cerebras", "https://api.cerebras.ai/v1", "CEREBRAS_API_KEY"],
    ["mistral", "Mistral", "https://api.mistral.ai/v1", "MISTRAL_API_KEY"],
    ["together", "Together", "https://api.together.xyz/v1", "TOGETHER_API_KEY"],
    ["fireworks", "Fireworks", "https://api.fireworks.ai/inference/v1", "FIREWORKS_API_KEY"],
    ["moonshot", "Moonshot", "https://api.moonshot.ai/v1", "MOONSHOT_API_KEY"],
    ["minimax", "MiniMax", "https://api.minimax.io/v1", "MINIMAX_API_KEY"],
    ["nvidia", "NVIDIA", "https://integrate.api.nvidia.com/v1", "NVIDIA_API_KEY"],
    ["siliconflow", "SiliconFlow", "https://api.siliconflow.cn/v1", "SILICONFLOW_API_KEY"],
    ["zhipu-bigmodel", "Zhipu", "https://open.bigmodel.cn/api/paas/v4", "ZHIPU_API_KEY"],
    ["volcengine", "Volcengine", "https://ark.cn-beijing.volces.com/api/v3", "ARK_API_KEY"],
    ["deepinfra", "DeepInfra", "https://api.deepinfra.com/v1/openai", "DEEPINFRA_API_KEY"],
    ["novita", "Novita", "https://api.novita.ai/openai/v1", "NOVITA_API_KEY"],
    ["sambanova", "SambaNova", "https://api.sambanova.ai/v1", "SAMBANOVA_API_KEY"],
    ["stepfun", "StepFun", "https://api.stepfun.com/v1", "STEPFUN_API_KEY"],
    ["ollama", "Ollama", "http://localhost:11434/v1", null],
    ["vllm", "vLLM", "http://localhost:8000/v1", null],
    ["lm-studio", "LM Studio", "http://localhost:1234/v1", null],
    ["litellm", "LiteLLM", "http://localhost:4000/v1", null],
  ] as [string, string, string, string | null, string?][]
).map(([id, displayName, baseUrl, env, adapter]) => ({ id, displayName, adapter: adapter ?? "openai-chat", baseUrl, apiKey: env ? `env:${env}` : null }));

// ---------------------------------------------------------------- providers & models

type ProviderSeed = {
  id: string;
  preset: string | null;
  apiKey: string;
  keyStatus: ProviderInfo["keyStatus"];
  keyMessage?: string;
  models: string[];
  enabled?: boolean;
  /** Custom endpoints only. */
  custom?: { displayName: string; adapter: string; baseUrl: string; auth?: string };
};

const PROVIDER_SEEDS: ProviderSeed[] = [
  { id: "anthropic", preset: "anthropic", apiKey: "keyring:anthropic", keyStatus: "ok", models: ["claude-opus-5-5", "claude-sonnet-5", "claude-haiku-4-5"] },
  { id: "openai", preset: "openai", apiKey: "keyring:openai", keyStatus: "ok", models: ["gpt-5.5", "gpt-5.5-mini", "gpt-5.3-codex"] },
  { id: "deepseek", preset: "deepseek", apiKey: "env:DEEPSEEK_API_KEY", keyStatus: "ok", models: ["deepseek-chat", "deepseek-reasoner"] },
  { id: "openrouter", preset: "openrouter", apiKey: "keyring:openrouter", keyStatus: "ok", models: ["google/gemini-3-pro", "qwen/qwen3-coder"] },
  {
    id: "moonshot",
    preset: "moonshot",
    apiKey: "env:MOONSHOT_API_KEY",
    keyStatus: "missing",
    keyMessage: "environment variable `MOONSHOT_API_KEY` is not set",
    models: ["kimi-k2.5"],
  },
  { id: "xai", preset: "xai", apiKey: "keyring:xai", keyStatus: "ok", models: ["grok-4.2", "grok-code-fast-1"] },
  { id: "zhipu-bigmodel", preset: "zhipu-bigmodel", apiKey: "keyring:zhipu-bigmodel", keyStatus: "ok", models: ["glm-5"] },
  { id: "minimax", preset: "minimax", apiKey: "keyring:minimax", keyStatus: "ok", models: ["MiniMax-M2.5"] },
  { id: "siliconflow", preset: "siliconflow", apiKey: "keyring:siliconflow", keyStatus: "ok", models: ["Qwen/Qwen3-235B-A22B"] },
  { id: "groq", preset: "groq", apiKey: "keyring:groq", keyStatus: "ok", models: ["llama-3.3-70b-versatile"], enabled: false },
  { id: "ollama", preset: "ollama", apiKey: "none", keyStatus: "none", models: ["llama3.1:8b"] },
  {
    id: "team-relay",
    preset: null,
    apiKey: "env:TEAM_RELAY_KEY",
    keyStatus: "ok",
    models: ["qwen3-max"],
    custom: { displayName: "team-relay", adapter: "openai-chat", baseUrl: "https://llm.example.com/v1", auth: "bearer" },
  },
];

export const providers: ProviderInfo[] = PROVIDER_SEEDS.map((s) => {
  const preset = presets.find((p) => p.id === s.preset);
  const enabled = s.enabled ?? true;
  return {
    id: s.id,
    displayName: s.custom?.displayName ?? preset?.displayName ?? s.id,
    adapter: s.custom?.adapter ?? preset?.adapter ?? "openai-chat",
    baseUrl: s.custom?.baseUrl ?? preset?.baseUrl ?? "",
    preset: s.preset,
    enabled,
    apiKey: s.apiKey,
    keyStatus: s.keyStatus,
    keyMessage: s.keyMessage ?? null,
    models: s.models,
    raw: {
      preset: null,
      adapter: s.custom?.adapter ?? null,
      baseUrl: s.custom?.baseUrl ?? null,
      apiKey: s.apiKey,
      inlineKey: false,
      auth: s.custom?.auth ?? null,
      models: s.models,
      enabled,
    },
  };
});

type ModelSeed = [id: string, provider: string, displayName: string | null, price: Price | null, context: number | null];
const p = (input: number, output: number, cache_read?: number, cache_write?: number): Price => ({ input, output, cache_read: cache_read ?? null, cache_write: cache_write ?? null });

const MODEL_SEEDS: ModelSeed[] = [
  ["claude-opus-5-5", "anthropic", "Claude Opus 5.5", p(5, 25, 0.5, 6.25), 200_000],
  ["claude-sonnet-5", "anthropic", "Claude Sonnet 5", p(3, 15, 0.3, 3.75), 200_000],
  ["claude-haiku-4-5", "anthropic", "Claude Haiku 4.5", p(1, 5, 0.1, 1.25), 200_000],
  ["gpt-5.5", "openai", "GPT-5.5", p(1.25, 10, 0.125), 400_000],
  ["gpt-5.5-mini", "openai", "GPT-5.5 mini", p(0.25, 2, 0.025), 400_000],
  ["gpt-5.3-codex", "openai", "GPT-5.3 Codex", p(1.25, 10, 0.125), 400_000],
  ["deepseek-chat", "deepseek", "DeepSeek V3.2", p(0.28, 0.42, 0.028), 128_000],
  ["deepseek-reasoner", "deepseek", "DeepSeek R1", p(0.55, 2.19, 0.14), 128_000],
  ["google/gemini-3-pro", "openrouter", "Gemini 3 Pro", p(2, 12, 0.2), 1_000_000],
  ["qwen/qwen3-coder", "openrouter", "Qwen3 Coder", p(0.3, 1.2), 262_144],
  ["kimi-k2.5", "moonshot", "Kimi K2.5", p(0.6, 2.5, 0.15), 256_000],
  ["grok-4.2", "xai", "Grok 4.2", p(3, 15, 0.75), 256_000],
  ["grok-code-fast-1", "xai", "Grok Code Fast", p(0.2, 1.5, 0.02), 256_000],
  ["glm-5", "zhipu-bigmodel", "GLM-5", p(0.6, 2.2, 0.11), 200_000],
  ["MiniMax-M2.5", "minimax", "MiniMax M2.5", p(0.3, 1.2, 0.03), 204_800],
  ["Qwen/Qwen3-235B-A22B", "siliconflow", "Qwen3 235B", null, 131_072],
  ["llama-3.3-70b-versatile", "groq", "Llama 3.3 70B", p(0.59, 0.79), 131_072],
  ["llama3.1:8b", "ollama", "Llama 3.1 8B", null, 131_072],
  ["qwen3-max", "team-relay", null, null, null],
];

const ALIASES: Record<string, Record<string, string>> = { "claude-sonnet-5": { codex_desktop: "sonnet" }, "gpt-5.5": { claude_code: "claude-owo--gpt-5.5" } };

export const models: ModelInfo[] = MODEL_SEEDS.map(([id, provider, displayName, price, contextWindow]) => ({
  id,
  displayName: displayName ?? id,
  provider,
  upstreamModel: id,
  aliases: ALIASES[id] ?? {},
  price,
  contextWindow,
  available: providers.find((x) => x.id === provider)?.enabled ?? false,
  configured: displayName !== null || price !== null,
}));

// ---------------------------------------------------------------- apps

const APPS: [app: string, clientId: string, about: string, connected: boolean, hint: string | null, takesModel: boolean][] = [
  ["codex", "codex", "Codex CLI (profile `codex -p owo`)", true, "start with: codex -p owo", true],
  ["codex-desktop", "codex_desktop", "Codex Desktop (config.toml)", true, null, true],
  ["claude", "claude_code", "Claude Code (CLI and IDE extensions)", true, null, true],
  ["claude-desktop", "claude_desktop", "Claude Desktop (third-party inference mode)", false, null, true],
  ["cursor", "cursor", "Cursor (OwO AI Gateway models next to Cursor's own)", true, "active while the gateway runs", false],
  ["grok", "grok_build", "Grok Build (`grok`)", true, null, false],
  ["opencode", "opencode", "OpenCode", true, null, false],
  ["mcode", "minimax_code", "MiniMax Code", false, null, false],
  ["zcode", "zcode", "ZCode", false, null, false],
  ["copilot", "copilot_app", "GitHub Copilot app", false, null, false],
];

export const apps: AppInfo[] = APPS.map(([app, client_id, about, connected, hint, takes_model]) => ({ app, about, client_id, connected, hint, takes_model }));

export const general: General = {
  name: "OwO",
  listen: "127.0.0.1:8787",
  clients: { codex: { model: "gpt-5.3-codex" }, codex_desktop: { model: "claude-sonnet-5" }, claude_code: { model: "claude-opus-5-5" } },
};

// ---------------------------------------------------------------- call history (one year)

type Client = "claude_code" | "cursor" | "codex_desktop" | "codex" | "opencode" | "grok_build" | "claude_desktop" | "zcode" | "minimax_cli" | "copilot_app";

const MODEL_MIX: Record<Client, Record<string, number>> = {
  claude_code: { "claude-sonnet-5": 40, "claude-opus-5-5": 24, "claude-haiku-4-5": 9, "deepseek-chat": 8, "glm-5": 7, "kimi-k2.5": 6, "gpt-5.5": 4 },
  cursor: { "claude-sonnet-5": 30, "gpt-5.5": 24, "google/gemini-3-pro": 20, "grok-code-fast-1": 14, "claude-opus-5-5": 12 },
  codex_desktop: { "gpt-5.5": 38, "gpt-5.3-codex": 30, "claude-sonnet-5": 22, "deepseek-reasoner": 10 },
  codex: { "gpt-5.3-codex": 45, "gpt-5.5-mini": 22, "deepseek-chat": 15, "qwen3-max": 10, "llama3.1:8b": 8 },
  opencode: { "kimi-k2.5": 24, "glm-5": 22, "qwen/qwen3-coder": 18, "MiniMax-M2.5": 14, "Qwen/Qwen3-235B-A22B": 10, "deepseek-chat": 8, "llama-3.3-70b-versatile": 4 },
  grok_build: { "grok-4.2": 55, "grok-code-fast-1": 45 },
  claude_desktop: { "claude-opus-5-5": 60, "claude-sonnet-5": 40 },
  zcode: { "glm-5": 100 },
  minimax_cli: { "MiniMax-M2.5": 100 },
  copilot_app: { "gpt-5.5": 50, "claude-sonnet-5": 50 },
};

/** App shares, `back` days ago: some apps arrived later, some were used for a while only. */
function appMix(back: number): Partial<Record<Client, number>> {
  const mix: Partial<Record<Client, number>> = { claude_code: 30, cursor: 21, codex_desktop: 14, codex: 10, opencode: 9, minimax_cli: 2 };
  if (back < 160) mix.grok_build = 5;
  if (back > 35) mix.claude_desktop = 5;
  if (back > 70 && back < 290) mix.zcode = 6;
  if (back < 90) mix.copilot_app = 7;
  return mix;
}

const AGENTIC = new Set<Client>(["claude_code", "cursor", "codex_desktop", "codex", "opencode", "grok_build", "zcode", "copilot_app"]);
const REASONING = new Set(["claude-opus-5-5", "gpt-5.5", "gpt-5.3-codex", "deepseek-reasoner", "google/gemini-3-pro", "grok-4.2", "kimi-k2.5", "glm-5"]);
const NO_CACHE = new Set(["qwen/qwen3-coder", "Qwen/Qwen3-235B-A22B", "llama3.1:8b", "llama-3.3-70b-versatile", "qwen3-max"]);
const SPEED: Record<string, number> = { "grok-code-fast-1": 160, "claude-haiku-4-5": 140, "gpt-5.5-mini": 130, "llama-3.3-70b-versatile": 260, "llama3.1:8b": 38, "claude-opus-5-5": 55 };

const modelInfo = (id: string) => models.find((m) => m.id === id)!;

function cost(model: string, input: number, cached: number, cacheWrite: number, output: number): number | null {
  const price = modelInfo(model).price;
  if (!price) return null;
  const fresh = Math.max(0, input - cached - cacheWrite);
  return (fresh * price.input + cached * (price.cache_read ?? price.input) + cacheWrite * (price.cache_write ?? price.input) + output * price.output) / 1e6;
}

const pad = (n: number) => String(n).padStart(2, "0");
export const dayKey = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const timeKey = (d: Date) => `${dayKey(d)} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;

/** Working hours with a late-morning and an afternoon peak, and a quieter evening. */
function hourOfDay(): number {
  const r = rng();
  if (r < 0.34) return between(9.2, 12.4);
  if (r < 0.8) return between(13.6, 18.6);
  if (r < 0.95) return between(19.5, 23.4);
  return between(7.5, 9.2);
}

type Draft = Omit<StoredCall, "id"> & { at: number };

function makeCall(at: Date, client: Client, model: string): Draft {
  const info = modelInfo(model);
  const anthropic = info.provider === "anthropic";
  const agentic = AGENTIC.has(client);
  const input = agentic ? skewed(46_000, 0.55, 3_200, 190_000) : skewed(4_200, 0.7, 300, 30_000);
  const cached = NO_CACHE.has(model) ? 0 : Math.round(input * (agentic ? between(0.55, 0.93) : between(0, 0.45)));
  const cacheWrite = anthropic ? Math.round(input * between(0.02, 0.09)) : 0;
  const output = agentic ? skewed(1_150, 0.6, 60, 14_000) : skewed(700, 0.6, 40, 5_000);
  const reasoning = REASONING.has(model) ? Math.round(output * between(0.15, 0.6)) : 0;
  const first = int(model === "llama3.1:8b" ? 900 : 380, REASONING.has(model) ? 4_800 : 2_200);
  const duration = first + Math.round((output / (SPEED[model] ?? 85)) * 1000 * between(0.8, 1.25));
  const openaiStyle = !anthropic;
  const toolTurn = agentic && rng() < 0.62;
  return {
    at: at.getTime(),
    time: timeKey(at),
    request_id: `req_${hex(32)}`,
    client,
    requested_model: client === "claude_code" && !model.startsWith("claude") ? `claude-owo--${model}` : client === "codex_desktop" && model === "claude-sonnet-5" && rng() < 0.5 ? "sonnet" : model,
    model,
    provider: info.provider,
    upstream_model: info.upstreamModel,
    stream: rng() < 0.96,
    status: "ok",
    error_kind: null,
    error_message: null,
    upstream_status: null,
    duration_ms: duration,
    first_token_ms: first,
    input_tokens: input,
    cached_input_tokens: cached,
    cache_creation_input_tokens: anthropic ? cacheWrite : null,
    output_tokens: output,
    reasoning_tokens: REASONING.has(model) ? reasoning : null,
    cost_usd: cost(model, input, cached, cacheWrite, output),
    stop_reason: toolTurn ? (openaiStyle ? "tool_calls" : "tool_use") : openaiStyle ? "stop" : "end_turn",
  };
}

const FAILURES: [kind: string, status: number | null, message: (d: Draft) => string][] = [
  ["rate_limited", 429, (d) => `upstream returned 429 Too Many Requests: rate limit reached for ${d.upstream_model}, retry after 20s`],
  ["provider_unavailable", 503, () => "upstream returned 503 Service Unavailable: the server is overloaded, try again shortly"],
  ["timeout", null, () => "no response headers within 120s"],
  ["upstream_invalid_response", 502, () => "stream ended before `message_stop` (502 Bad Gateway from upstream)"],
  ["context_exceeded", 400, (d) => `prompt is too long: ${(d.input_tokens ?? 0) + 214_000} tokens > ${modelInfo(d.model ?? "").contextWindow ?? 200_000} maximum`],
];

function fail(d: Draft, kind: string, upstream: number | null, message: string): Draft {
  return {
    ...d,
    status: "error",
    error_kind: kind,
    upstream_status: upstream,
    error_message: message,
    input_tokens: null,
    cached_input_tokens: null,
    cache_creation_input_tokens: null,
    output_tokens: null,
    reasoning_tokens: null,
    cost_usd: null,
    stop_reason: null,
    first_token_ms: null,
    duration_ms: int(180, 2_400),
  };
}

function cancel(d: Draft): Draft {
  return { ...d, status: "cancelled", output_tokens: null, reasoning_tokens: null, cost_usd: null, stop_reason: null, duration_ms: Math.round(d.duration_ms * 0.4) };
}

function generate(now: Date): StoredCall[] {
  const drafts: Draft[] = [];
  const DAYS = 365;
  for (let back = DAYS - 1; back >= 0; back--) {
    const day = new Date(now);
    day.setHours(0, 0, 0, 0);
    day.setDate(day.getDate() - back);
    const weekend = day.getDay() === 0 || day.getDay() === 6;
    // Two holidays, a few random days off, and the weeks before the first day of use.
    if (back > 338 || (back >= 196 && back <= 204) || (back >= 96 && back <= 99)) continue;
    if (rng() < (weekend ? 0.3 : 0.035)) continue;
    const ramp = 0.3 + 0.7 * Math.min(1, (DAYS - back) / 260);
    const sprint = Math.floor(back / 11) % 4 === 0 ? 1.55 : 1;
    let n = (weekend ? between(3, 20) : between(34, 96)) * ramp * sprint;
    const cutoff = back === 0 ? now.getHours() + now.getMinutes() / 60 : 24;
    if (back === 0) n *= Math.min(1, cutoff / 19);
    for (let i = 0; i < Math.round(n); i++) {
      let hour = hourOfDay();
      if (hour >= cutoff) hour = between(8, cutoff - 0.05);
      const at = new Date(day.getTime() + hour * 3_600_000);
      const client = pick(appMix(back));
      let model = pick(MODEL_MIX[client]);
      // The Moonshot key went missing a few days ago; Groq was switched off last month.
      if (model === "kimi-k2.5" && back < 4) model = "glm-5";
      if (model === "llama-3.3-70b-versatile" && back < 35) model = "Qwen/Qwen3-235B-A22B";
      let d = makeCall(at, client, model);
      const r = rng();
      if (r < 0.011) {
        const [kind, code, message] = FAILURES[Math.floor(rng() * FAILURES.length)];
        d = fail(d, kind, code, message(d));
      } else if (r < 0.015) d = cancel(d);
      drafts.push(d);
    }
  }
  drafts.sort((a, b) => a.at - b.at);

  // The most recent calls, as the History page lists them (newest first): a rich Opus call on
  // row 2, a rate limit, the missing Moonshot key, a cancelled call, and a timeout.
  const at = (i: number) => new Date(drafts[drafts.length - 1 - i].at);
  const set = (i: number, d: Draft) => (drafts[drafts.length - 1 - i] = d);
  set(0, makeCall(at(0), "cursor", "claude-sonnet-5"));
  const rich = makeCall(at(1), "claude_code", "claude-opus-5-5");
  const input = 128_406;
  const cached = 109_872;
  const write = 6_214;
  const output = 3_918;
  set(1, {
    ...rich,
    requested_model: "claude-opus-5-5",
    input_tokens: input,
    cached_input_tokens: cached,
    cache_creation_input_tokens: write,
    output_tokens: output,
    reasoning_tokens: 1_402,
    first_token_ms: 1_864,
    duration_ms: 52_730,
    stream: true,
    stop_reason: "tool_use",
    cost_usd: cost("claude-opus-5-5", input, cached, write, output),
  });
  set(2, fail(makeCall(at(2), "cursor", "google/gemini-3-pro"), "rate_limited", 429, "upstream returned 429 Too Many Requests: rate limit reached for google/gemini-3-pro, retry after 20s"));
  set(3, makeCall(at(3), "codex_desktop", "gpt-5.5"));
  set(4, makeCall(at(4), "opencode", "glm-5"));
  set(5, fail(makeCall(at(5), "claude_code", "kimi-k2.5"), "authentication_failed", null, "environment variable `MOONSHOT_API_KEY` is not set"));
  set(6, makeCall(at(6), "grok_build", "grok-code-fast-1"));
  set(8, cancel(makeCall(at(8), "codex_desktop", "gpt-5.5")));
  set(12, fail(makeCall(at(12), "codex", "deepseek-chat"), "timeout", null, "no response headers within 120s"));

  return drafts.map(({ at: _at, ...call }, i) => ({ id: i + 1, ...call }));
}

export const now = new Date();
export const calls: StoredCall[] = generate(now);

// ---------------------------------------------------------------- MCP

const MCP_APPS: [app: string, label: string, installed: boolean, file: string | null, transports: McpAppInfo["transports"], cwd: boolean][] = [
  ["codex", "Codex", true, `${HOME}/.codex/config.toml`, ["stdio", "http"], true],
  ["claude", "Claude Code", true, `${HOME}/.claude.json`, ["stdio", "http", "sse"], false],
  ["claude-desktop", "Claude Desktop", true, `${HOME}/.config/Claude/claude_desktop_config.json`, ["stdio"], false],
  ["cursor", "Cursor", true, `${HOME}/.cursor/mcp.json`, ["stdio", "http", "sse"], false],
  ["opencode", "OpenCode", true, `${HOME}/.config/opencode/opencode.json`, ["stdio", "http", "sse"], false],
  ["grok", "Grok Build", true, `${HOME}/.grok/config.toml`, ["stdio", "http", "sse"], true],
  ["mcode", "MiniMax Code", false, null, ["stdio", "http", "sse"], false],
  ["zcode", "ZCode", true, `${HOME}/.zcode/cli/config.json`, ["stdio", "http", "sse"], false],
  ["copilot", "GitHub Copilot", true, `${HOME}/.copilot/mcp-config.json`, ["stdio", "http", "sse"], false],
];

export const mcpApps: McpAppInfo[] = [
  ...MCP_APPS.map(([app, label, installed, file, transports, cwd]) => ({ app, label, supported: true, installed, files: file ? [file] : [], transports, cwd, reason: null })),
  { app: "mmx", label: "MiniMax CLI", supported: false, installed: false, files: [], transports: [], cwd: false, reason: "the MiniMax CLI (`mmx`) runs single text commands and has no MCP support" },
];

const ref = (key: string, value: string, missing: string | null = null): McpKeyValue => ({ key, value, reference: true, missing, secret: true });
const plain = (key: string, value: string): McpKeyValue => ({ key, value, reference: false, missing: null, secret: false });

type ServerSeed = Omit<McpServer, "apps" | "summary"> & { on: Record<string, McpAppState["state"]> };

const SERVER_SEEDS: ServerSeed[] = [
  {
    name: "filesystem",
    transport: "stdio",
    description: tr("Read and write files under ~/projects", "读写 ~/projects 下的文件"),
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-filesystem", "~/projects"],
    cwd: null,
    url: null,
    env: [],
    headers: [],
    on: { codex: "synced", claude: "synced", cursor: "synced", opencode: "synced", grok: "synced" },
  },
  {
    name: "github",
    transport: "stdio",
    description: tr("Issues, pull requests and code search on GitHub", "GitHub 的 Issue、PR 与代码搜索"),
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-github"],
    cwd: null,
    url: null,
    env: [ref("GITHUB_PERSONAL_ACCESS_TOKEN", "keyring:mcp-github-GITHUB_PERSONAL_ACCESS_TOKEN")],
    headers: [],
    on: { codex: "synced", claude: "synced", cursor: "synced", grok: "outdated", copilot: "synced" },
  },
  {
    name: "playwright",
    transport: "stdio",
    description: tr("Drive a real browser: navigate, click, fill forms, take screenshots", "操控真实浏览器：打开页面、点击、填表、截图"),
    command: "npx",
    args: ["@playwright/mcp@latest"],
    cwd: null,
    url: null,
    env: [],
    headers: [],
    on: { claude: "synced", cursor: "synced", codex: "synced", opencode: "synced" },
  },
  {
    name: "context7",
    transport: "http",
    description: tr("Up-to-date library documentation for the prompt", "为提示词提供最新的库文档"),
    command: null,
    args: [],
    cwd: null,
    url: "https://mcp.context7.com/mcp",
    env: [],
    headers: [ref("CONTEXT7_API_KEY", "keyring:mcp-context7-CONTEXT7_API_KEY")],
    on: { cursor: "synced", claude: "synced", opencode: "synced", zcode: "synced", codex: "synced" },
  },
  {
    name: "sequential-thinking",
    transport: "stdio",
    description: tr("A scratchpad tool for step-by-step problem solving", "分步推理用的草稿工具"),
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-sequential-thinking"],
    cwd: null,
    url: null,
    env: [],
    headers: [],
    on: { claude: "synced", "claude-desktop": "synced", cursor: "synced" },
  },
  {
    name: "postgres",
    transport: "stdio",
    description: tr("Read-only SQL against the local development database", "对本地开发库执行只读 SQL"),
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-postgres", "postgresql://localhost:5432/app_dev"],
    cwd: `${HOME}/projects/app`,
    url: null,
    env: [plain("PGUSER", "dev"), ref("PGPASSWORD", "keyring:mcp-postgres-PGPASSWORD")],
    headers: [],
    on: { codex: "synced", grok: "modified" },
  },
  {
    name: "linear",
    transport: "http",
    description: tr("Linear issues and projects", "Linear 的任务与项目"),
    command: null,
    args: [],
    cwd: null,
    url: "https://mcp.linear.app/mcp",
    env: [],
    headers: [ref("Authorization", "env:LINEAR_AUTH", "environment variable `LINEAR_AUTH` is not set")],
    on: { cursor: "pending" },
  },
];

export const mcpServers: McpServer[] = SERVER_SEEDS.map(({ on, ...s }) => ({
  ...s,
  summary: s.transport === "stdio" ? [s.command, ...s.args].join(" ") : `${s.url}${s.transport === "sse" ? " (sse)" : ""}`,
  apps: MCP_APPS.map(([app]) => ({ app, enabled: app in on, state: on[app] ?? "off", unsupported: null })),
}));

const view = (s: Partial<McpServer> & Pick<McpServer, "transport">) => ({
  description: null,
  command: null,
  args: [],
  cwd: null,
  url: null,
  env: [],
  headers: [],
  summary: s.transport === "stdio" ? [s.command, ...(s.args ?? [])].join(" ") : (s.url ?? ""),
  ...s,
});

export const mcpFound: McpFound[] = [
  ...mcpServers.flatMap((s) =>
    s.apps.filter((a) => a.enabled).map((a) => ({ app: a.app, file: MCP_APPS.find((x) => x[0] === a.app)?.[3] ?? "", name: s.name, managed: true, relation: "same" as const, server: s, error: null, dropped: [], secrets: [] })),
  ),
  { app: "cursor", file: `${HOME}/.cursor/mcp.json`, name: "fetch", managed: false, relation: "new", server: view({ transport: "stdio", command: "uvx", args: ["mcp-server-fetch"] }), error: null, dropped: [], secrets: [] },
  { app: "claude", file: `${HOME}/.claude.json`, name: "memory", managed: false, relation: "new", server: view({ transport: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-memory"] }), error: null, dropped: [], secrets: [] },
  { app: "codex", file: `${HOME}/.codex/config.toml`, name: "memory", managed: false, relation: "same", server: view({ transport: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-memory"] }), error: null, dropped: ["startup_timeout_sec"], secrets: [] },
  {
    app: "claude-desktop",
    file: `${HOME}/.config/Claude/claude_desktop_config.json`,
    name: "brave-search",
    managed: false,
    relation: "new",
    server: view({ transport: "stdio", command: "npx", args: ["-y", "@brave/brave-search-mcp-server"], env: [{ key: "BRAVE_API_KEY", value: null, reference: false, missing: null, secret: true }] }),
    error: null,
    dropped: [],
    secrets: ["BRAVE_API_KEY"],
  },
  { app: "cursor", file: `${HOME}/.cursor/mcp.json`, name: "notion", managed: false, relation: "new", server: view({ transport: "http", url: "https://mcp.notion.com/mcp" }), error: null, dropped: [], secrets: [] },
  { app: "opencode", file: `${HOME}/.config/opencode/opencode.json`, name: "notion", managed: false, relation: "different", server: view({ transport: "sse", url: "https://mcp.notion.com/sse" }), error: null, dropped: [], secrets: [] },
];

// ---------------------------------------------------------------- Skills

const STORE = `${HOME}/.agents/skills`;

export const skillApps: SkillApp[] = [
  { id: "codex", name: "Codex", support: "switch", detected: true, dir: STORE, switch_file: `${HOME}/.codex/config.toml`, note: "reads ~/.agents/skills; off = `[[skills.config]] enabled = false` in config.toml" },
  { id: "claude", name: "Claude Code", support: "link", detected: true, dir: `${HOME}/.claude/skills`, switch_file: null, note: "reads only ~/.claude/skills; on = a link there to the skill in ~/.agents/skills" },
  { id: "cursor", name: "Cursor", support: "always-on", detected: true, dir: STORE, switch_file: null, note: "reads ~/.agents/skills (and ~/.claude/skills, ~/.codex/skills); no per-skill setting OwO AI Gateway may edit" },
  { id: "opencode", name: "OpenCode", support: "switch", detected: true, dir: STORE, switch_file: `${HOME}/.config/opencode/opencode.json`, note: 'reads ~/.agents/skills; off = `permission.skill.<name> = "deny"` in opencode.json' },
  { id: "grok", name: "Grok Build", support: "switch", detected: true, dir: STORE, switch_file: `${HOME}/.grok/config.toml`, note: "reads ~/.agents/skills; off = `[skills] disabled` in ~/.grok/config.toml" },
  { id: "zcode", name: "ZCode", support: "always-on", detected: true, dir: STORE, switch_file: null, note: "reads ~/.agents/skills; turn skills off in ZCode's Settings → Skills" },
  { id: "copilot", name: "GitHub Copilot", support: "switch", detected: false, dir: STORE, switch_file: `${HOME}/.copilot/settings.json`, note: "reads ~/.agents/skills; off = `disabledSkills` in ~/.copilot/settings.json (not installed)" },
  { id: "mcode", name: "MiniMax Code", support: "unsupported", detected: false, dir: null, switch_file: null, note: "loads skills only from MiniMax Code plugins (not installed)" },
  { id: "claude-desktop", name: "Claude Desktop", support: "unsupported", detected: true, dir: null, switch_file: null, note: "skills are uploaded to your claude.ai account, not read from disk" },
];

/** On / off per switchable app (`codex`, `claude`, `opencode`, `grok`, `copilot`); Cursor and ZCode are always on. */
function skillStates(on: Partial<Record<SkillAppId, boolean>>): SkillAppState[] {
  const linked = on.claude ?? true;
  const sw = (app: SkillAppId, dup = false): SkillAppState => ({ app, enabled: on[app] ?? true, can_toggle: true, state: (on[app] ?? true) ? "on" : "off-by-owo", duplicate: dup && linked });
  return [
    sw("codex"),
    { app: "claude", enabled: linked, can_toggle: true, state: linked ? "linked" : "not-linked", duplicate: false },
    { app: "cursor", enabled: true, can_toggle: false, state: "always-on", duplicate: linked },
    sw("opencode", true),
    sw("grok", true),
    { app: "zcode", enabled: true, can_toggle: false, state: "always-on", duplicate: false },
    sw("copilot"),
  ];
}

const DAY = 86_400;
const unix = Math.floor(now.getTime() / 1000);

type SkillSeed = [name: string, description: string, source: Skill["source"], on: Partial<Record<SkillAppId, boolean>>, extra?: Partial<Skill>];

const gh = (repo: string, path: string, commit: string): Skill["source"] => ({ kind: "github", label: `github:${repo}/${path}`, repo, commit });

const SKILL_SEEDS: SkillSeed[] = [
  ["pdf", "Read, fill and assemble PDF files: extract text and tables, complete form fields, merge and split documents.", gh("anthropics/skills", "skills/pdf", "8d2f41c7a0e9b53f6c1d24e8a7b90f3e5c6d1a42"), { grok: false }],
  [
    "frontend-design",
    "Build distinctive, production-grade web interfaces: pick a clear visual direction, then implement it with care for typography, spacing and motion.",
    gh("anthropics/skills", "skills/frontend-design", "8d2f41c7a0e9b53f6c1d24e8a7b90f3e5c6d1a42"),
    { opencode: false },
    { update: { latest_commit: "f41b09d3c2e8a7651d4b3c2a1f0e9d8c7b6a5f43", available: true, missing: false, checked_at: unix - 1_800 } },
  ],
  ["webapp-testing", "Test a local web app end to end with a headless browser: start the server, script the flow, capture screenshots and console logs.", gh("anthropics/skills", "skills/webapp-testing", "8d2f41c7a0e9b53f6c1d24e8a7b90f3e5c6d1a42"), { codex: true }],
  ["react-best-practices", "Performance and correctness rules for React and Next.js code: data fetching, rendering, bundle size and server components.", gh("vercel-labs/agent-skills", "skills/react-best-practices", "2c7e9a1b4d6f8035e2a1c9b7d5f3e1a0c8b6d4f2"), { grok: false, copilot: false }],
  [
    "release-notes",
    tr("Draft release notes from merged pull requests and the changelog, grouped by area and written in the project's voice.", "根据已合并的 PR 和变更日志起草发布说明，按模块分组，保持项目一贯的语气。"), { kind: "authored", label: "hand-written" }, {}, { modified: false }],
  [
    "api-style-guide",
    tr("The team's REST conventions: resource naming, pagination, error bodies and versioning, with examples to copy.", "团队的 REST 约定：资源命名、分页、错误响应和版本管理，附可直接照抄的示例。"), { kind: "local", label: `${HOME}/work/skills/api-style-guide` }, { opencode: false, grok: false }],
  [
    "brand-voice",
    tr("Write in the product's voice: short sentences, concrete verbs, no hype; includes a glossary of preferred terms.", "用产品的语气写作：短句、具体的动词、不夸张；附推荐用语表。"), { kind: "zip", label: `${HOME}/Downloads/brand-voice.zip` }, { codex: false, claude: false }],
  ["hugging-face-datasets", "Find, load and inspect datasets on the Hugging Face Hub; stream large splits and push cleaned versions back.", gh("huggingface/skills", "skills/hugging-face-datasets", "5a3c1e9f7b2d4068a1c3e5f7b9d0a2c4e6f8b1d3"), { claude: false, opencode: true }],
];

export const skills: Skill[] = SKILL_SEEDS.map(([name, description, source, on, extra], i) => ({
  name,
  description,
  path: `${STORE}/${name}`,
  location: "agents",
  managed: true,
  source,
  installed_at: unix - (40 + i * 9) * DAY,
  updated_at: unix - (3 + i * 4) * DAY,
  modified: false,
  is_link: false,
  can_adopt: false,
  apps: skillStates(on),
  warnings: [],
  ...extra,
}));

const unmanaged = (name: string, description: string, location: Skill["location"], dir: string, canAdopt: boolean): Skill => ({
  name,
  description,
  path: `${dir}/${name}`,
  location,
  managed: false,
  modified: false,
  is_link: false,
  can_adopt: canAdopt,
  apps: [],
  warnings: [],
});

export const unmanagedSkills: Skill[] = [
  unmanaged("code-review", "Review a diff for correctness, security and readability; leave actionable comments.", "agents", STORE, true),
  unmanaged("tailwind-v4", "Tailwind CSS v4 idioms: @theme tokens, container queries, and migrating from v3.", "agents", STORE, true),
  unmanaged("commit-message", "Write Conventional Commits messages from the staged diff.", "claude", `${HOME}/.claude/skills`, false),
  unmanaged("sql-helper", "Explain and optimise SQL queries; suggest indexes.", "cursor", `${HOME}/.cursor/skills`, false),
  unmanaged("docx", "Create and edit Word documents with tracked changes.", "codex", `${HOME}/.codex/skills`, false),
];

export const skillStore = STORE;

type Found = [name: string, description: string, state?: "installed" | "update" | "conflict"];
const repo = (name: string, commit: string, found: Found[]): SkillRepo => ({
  repo: name,
  builtin: true,
  commit,
  fetched_at: unix - 3_600,
  skills: found.map(([skill, description, state]) => ({
    name: skill,
    description,
    dir: `skills/${skill}`,
    install: `github:${name.split("/").slice(0, 2).join("/")}/skills/${skill}`,
    installed: state === "installed" || state === "update",
    update_available: state === "update",
    conflict: state === "conflict",
  })),
});

export const skillRepos: SkillRepo[] = [
  repo("anthropics/skills/skills", "f41b09d3c2e8a7651d4b3c2a1f0e9d8c7b6a5f43", [
    ["algorithmic-art", "Generative art with p5.js: seeded randomness, flow fields and particle systems."],
    ["canvas-design", "Posters and visual pieces as PNG or PDF, composed with a clear design philosophy."],
    ["docx", "Create, edit and review Word documents, including tracked changes and comments.", "conflict"],
    ["frontend-design", "Distinctive, production-grade web interfaces with a clear visual direction.", "update"],
    ["mcp-builder", "Design and build MCP servers that give agents well-shaped tools."],
    ["pdf", "Extract text and tables, fill forms, merge and split PDF files.", "installed"],
    ["pptx", "Build and edit slide decks: layouts, speaker notes and charts."],
    ["skill-creator", "Write a new skill: SKILL.md frontmatter, references and scripts."],
    ["webapp-testing", "End-to-end tests of local web apps with a headless browser.", "installed"],
    ["xlsx", "Spreadsheets with formulas, formatting and charts; analyse existing workbooks."],
  ]),
  repo("openai/skills/skills", "b7e3a9c1d5f2084e6a1c3b5d7f9e0a2c4b6d8f1e", [
    ["gh-fix-ci", "Find why a GitHub Actions run failed and propose a fix."],
    ["openai-docs", "Answer questions from the current OpenAI API documentation."],
    ["playwright", "Automate a browser from the terminal for checks and screenshots."],
    ["spreadsheet", "Read, clean and summarise CSV and Excel files."],
  ]),
  repo("vercel-labs/agent-skills/skills", "2c7e9a1b4d6f8035e2a1c9b7d5f3e1a0c8b6d4f2", [
    ["react-best-practices", "Performance rules for React and Next.js code.", "installed"],
    ["web-design-guidelines", "Audit a UI against accessibility and interface guidelines."],
    ["vercel-deploy", "Deploy the current project and report the preview URL."],
  ]),
  repo("huggingface/skills/skills", "5a3c1e9f7b2d4068a1c3e5f7b9d0a2c4e6f8b1d3", [
    ["hugging-face-datasets", "Find, load and inspect datasets on the Hub.", "installed"],
    ["hugging-face-evaluation", "Evaluate a model on standard benchmarks and compare runs."],
    ["hugging-face-model-trainer", "Fine-tune a model with TRL and push it to the Hub."],
  ]),
  repo("MiniMax-AI/skills/skills", "9e1d3c5b7a9f2e4d6c8b0a1f3e5d7c9b2a4f6e8d", [
    ["frontend-dev", "Full-stack frontend work with a design-first workflow."],
    ["minimax-multimodal", "Generate speech, images and video with MiniMax models."],
  ]),
];

// ---------------------------------------------------------------- skill files (the editor)

export const skillTrees: Record<string, SkillTree> = {
  pdf: {
    path: `${STORE}/pdf`,
    more: 0,
    entries: [
      { path: "SKILL.md", depth: 0, dir: false, size: 3_412, link: false },
      { path: "forms.md", depth: 0, dir: false, size: 2_086, link: false },
      { path: "reference.md", depth: 0, dir: false, size: 5_730, link: false },
      { path: "LICENSE.txt", depth: 0, dir: false, size: 1_071, link: false },
      { path: "examples", depth: 0, dir: true, size: 0, link: false },
      { path: "examples/invoice.md", depth: 1, dir: false, size: 846, link: false },
      { path: "examples/tax-form.md", depth: 1, dir: false, size: 1_204, link: false },
      { path: "scripts", depth: 0, dir: true, size: 0, link: false },
      { path: "scripts/extract_tables.py", depth: 1, dir: false, size: 2_318, link: false },
      { path: "scripts/fill_form.py", depth: 1, dir: false, size: 3_902, link: false },
      { path: "scripts/merge.py", depth: 1, dir: false, size: 1_155, link: false },
    ],
  },
};

const PDF_SKILL = `---
name: pdf
description: Read, fill and assemble PDF files — extract text and tables, complete form fields, merge and split documents.
---

# PDF

Use this skill whenever the task involves a \`.pdf\` file.

## When to use

- Pull text or tables out of a report or statement
- Fill in a form (see [forms.md](forms.md) for field types)
- Merge several files, split one, or reorder its pages

## Steps

1. Inspect the file first: page count, whether it has a text layer, form fields.
2. For text and tables, run \`scripts/extract_tables.py <file>\` and check the CSV it writes.
3. For forms, list the fields, map them to the user's data, then run
   \`scripts/fill_form.py <file> <values.json>\`.
4. Save the result next to the input as \`<name>.filled.pdf\` and never overwrite the original.

## Notes

- Scanned pages have no text layer: say so and offer OCR instead of guessing.
- Keep page order unless the user asks otherwise.
- See [reference.md](reference.md) for the library calls the scripts use.
`;

export function skillFile(name: string, path?: string | null): SkillFile {
  const skill = skills.find((s) => s.name === name) ?? skills[0];
  const file = path ?? "SKILL.md";
  const content = name === "pdf" && file === "SKILL.md" ? PDF_SKILL : `# ${file.replace(/\.md$/i, "")}\n\nNotes for the \`${name}\` skill.\n`;
  return { name, path: skill.path, content, source: skill.source?.kind ?? "adopted", editable: true };
}

// ---------------------------------------------------------------- config.toml

export const configText = `# OwO AI Gateway: providers, models, and keys are defined once here and
# shared by every connected app. Keys are references, never the key itself.

name = "OwO"

[server]
listen = "127.0.0.1:8787"

[providers.anthropic]
api_key = "keyring:anthropic"
models = ["claude-opus-5-5", "claude-sonnet-5", "claude-haiku-4-5"]

[providers.openai]
api_key = "keyring:openai"
models = ["gpt-5.5", "gpt-5.5-mini", "gpt-5.3-codex"]

[providers.deepseek]
api_key = "env:DEEPSEEK_API_KEY"
models = ["deepseek-chat", "deepseek-reasoner"]

[providers.openrouter]
api_key = "keyring:openrouter"
models = ["google/gemini-3-pro", "qwen/qwen3-coder"]

[providers.moonshot]
api_key = "env:MOONSHOT_API_KEY"
models = ["kimi-k2.5"]

[providers.xai]
api_key = "keyring:xai"
models = ["grok-4.2", "grok-code-fast-1"]

[providers.zhipu-bigmodel]
api_key = "keyring:zhipu-bigmodel"
models = ["glm-5"]

[providers.groq]
api_key = "keyring:groq"
models = ["llama-3.3-70b-versatile"]
enabled = false

[providers.ollama]
api_key = "none"
models = ["llama3.1:8b"]

# A team relay that speaks the OpenAI Chat Completions protocol.
[providers.team-relay]
adapter = "openai-chat"
base_url = "https://llm.example.com/v1"
api_key = "env:TEAM_RELAY_KEY"
auth = "bearer"
models = ["qwen3-max"]

# Display names, aliases, and prices (USD per million tokens).
[[models]]
id = "claude-opus-5-5"
provider = "anthropic"
display_name = "Claude Opus 5.5"
price = { input = 5, output = 25, cache_read = 0.5, cache_write = 6.25 }

[[models]]
id = "claude-sonnet-5"
provider = "anthropic"
display_name = "Claude Sonnet 5"
aliases = { codex-desktop = "sonnet" }
price = { input = 3, output = 15, cache_read = 0.3, cache_write = 3.75 }

[[models]]
id = "gpt-5.5"
provider = "openai"
display_name = "GPT-5.5"
price = { input = 1.25, output = 10, cache_read = 0.125 }

[clients.codex]
model = "gpt-5.3-codex"

[clients.codex-desktop]
model = "claude-sonnet-5"

[clients.claude]
model = "claude-opus-5-5"

[mcp.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "keyring:mcp-github-GITHUB_PERSONAL_ACCESS_TOKEN" }
apps = ["claude", "codex", "cursor", "grok", "copilot"]

[mcp.context7]
url = "https://mcp.context7.com/mcp"
headers = { CONTEXT7_API_KEY = "keyring:mcp-context7-CONTEXT7_API_KEY" }
apps = ["claude", "codex", "cursor", "opencode", "zcode"]
`;
