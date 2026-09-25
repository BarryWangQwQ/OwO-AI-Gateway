import { Box, Code } from "@/components/icons";
import { cn } from "@/lib/utils";

// Brand marks from @lobehub/icons-static-svg (MIT). Colour variants
// (`*-color.svg`) are used wherever the package ships one; brands whose logo is
// genuinely monochrome (OpenAI, Anthropic, xAI/Grok, Groq, Ollama, Moonshot,
// LM Studio, Cursor, GitHub Copilot, OpenCode) come as `fill="currentColor"`
// SVGs so they follow the text colour in both themes. Importing `?raw` inlines
// only the icons listed here.
import anthropic from "@lobehub/icons-static-svg/icons/anthropic.svg?raw";
import azure from "@lobehub/icons-static-svg/icons/azure-color.svg?raw";
import baichuan from "@lobehub/icons-static-svg/icons/baichuan-color.svg?raw";
import bedrock from "@lobehub/icons-static-svg/icons/bedrock-color.svg?raw";
import cerebras from "@lobehub/icons-static-svg/icons/cerebras-color.svg?raw";
import claude from "@lobehub/icons-static-svg/icons/claude-color.svg?raw";
import cohere from "@lobehub/icons-static-svg/icons/cohere-color.svg?raw";
import cursor from "@lobehub/icons-static-svg/icons/cursor.svg?raw";
import deepinfra from "@lobehub/icons-static-svg/icons/deepinfra-color.svg?raw";
import deepseek from "@lobehub/icons-static-svg/icons/deepseek-color.svg?raw";
import doubao from "@lobehub/icons-static-svg/icons/doubao-color.svg?raw";
import fireworks from "@lobehub/icons-static-svg/icons/fireworks-color.svg?raw";
import gemini from "@lobehub/icons-static-svg/icons/gemini-color.svg?raw";
import gemma from "@lobehub/icons-static-svg/icons/gemma-color.svg?raw";
import githubcopilot from "@lobehub/icons-static-svg/icons/githubcopilot.svg?raw";
import google from "@lobehub/icons-static-svg/icons/google-color.svg?raw";
import grok from "@lobehub/icons-static-svg/icons/grok.svg?raw";
import groq from "@lobehub/icons-static-svg/icons/groq.svg?raw";
import hunyuan from "@lobehub/icons-static-svg/icons/hunyuan-color.svg?raw";
import kimi from "@lobehub/icons-static-svg/icons/kimi-color.svg?raw";
import lmstudio from "@lobehub/icons-static-svg/icons/lmstudio.svg?raw";
import meta from "@lobehub/icons-static-svg/icons/meta-color.svg?raw";
import minimax from "@lobehub/icons-static-svg/icons/minimax-color.svg?raw";
import mistral from "@lobehub/icons-static-svg/icons/mistral-color.svg?raw";
import moonshot from "@lobehub/icons-static-svg/icons/moonshot.svg?raw";
import novita from "@lobehub/icons-static-svg/icons/novita-color.svg?raw";
import nvidia from "@lobehub/icons-static-svg/icons/nvidia-color.svg?raw";
import ollama from "@lobehub/icons-static-svg/icons/ollama.svg?raw";
import openai from "@lobehub/icons-static-svg/icons/openai.svg?raw";
import opencode from "@lobehub/icons-static-svg/icons/opencode.svg?raw";
import openrouter from "@lobehub/icons-static-svg/icons/openrouter-color.svg?raw";
import perplexity from "@lobehub/icons-static-svg/icons/perplexity-color.svg?raw";
import qwen from "@lobehub/icons-static-svg/icons/qwen-color.svg?raw";
import sambanova from "@lobehub/icons-static-svg/icons/sambanova-color.svg?raw";
import siliconcloud from "@lobehub/icons-static-svg/icons/siliconcloud-color.svg?raw";
import stepfun from "@lobehub/icons-static-svg/icons/stepfun-color.svg?raw";
import together from "@lobehub/icons-static-svg/icons/together-color.svg?raw";
import vertexai from "@lobehub/icons-static-svg/icons/vertexai-color.svg?raw";
import vllm from "@lobehub/icons-static-svg/icons/vllm-color.svg?raw";
import volcengine from "@lobehub/icons-static-svg/icons/volcengine-color.svg?raw";
import wenxin from "@lobehub/icons-static-svg/icons/wenxin-color.svg?raw";
import xai from "@lobehub/icons-static-svg/icons/xai.svg?raw";
import yi from "@lobehub/icons-static-svg/icons/yi-color.svg?raw";
import zhipu from "@lobehub/icons-static-svg/icons/zhipu-color.svg?raw";

const SVGS = {
  anthropic,
  azure,
  baichuan,
  bedrock,
  cerebras,
  claude,
  cohere,
  cursor,
  deepinfra,
  deepseek,
  doubao,
  fireworks,
  gemini,
  gemma,
  githubcopilot,
  google,
  grok,
  groq,
  hunyuan,
  kimi,
  lmstudio,
  meta,
  minimax,
  mistral,
  moonshot,
  novita,
  nvidia,
  ollama,
  openai,
  opencode,
  openrouter,
  perplexity,
  qwen,
  sambanova,
  siliconcloud,
  stepfun,
  together,
  vertexai,
  vllm,
  volcengine,
  wenxin,
  xai,
  yi,
  zhipu,
} as const;

export type Vendor = keyof typeof SVGS;

/** Provider ids / preset ids (see registry/providers.toml) → vendor. */
const PROVIDERS: Record<string, Vendor> = {
  openai: "openai",
  azure: "azure",
  "azure-openai": "azure",
  anthropic: "anthropic",
  claude: "claude",
  bedrock: "bedrock",
  google: "google",
  gemini: "gemini",
  vertex: "vertexai",
  vertexai: "vertexai",
  openrouter: "openrouter",
  deepseek: "deepseek",
  xai: "xai",
  grok: "grok",
  groq: "groq",
  cerebras: "cerebras",
  mistral: "mistral",
  together: "together",
  fireworks: "fireworks",
  moonshot: "moonshot",
  kimi: "kimi",
  minimax: "minimax",
  nvidia: "nvidia",
  siliconflow: "siliconcloud",
  siliconcloud: "siliconcloud",
  zhipu: "zhipu",
  "zhipu-bigmodel": "zhipu",
  bigmodel: "zhipu",
  glm: "zhipu",
  volcengine: "volcengine",
  ark: "volcengine",
  doubao: "doubao",
  bytedance: "doubao",
  deepinfra: "deepinfra",
  novita: "novita",
  sambanova: "sambanova",
  stepfun: "stepfun",
  ollama: "ollama",
  vllm: "vllm",
  "lm-studio": "lmstudio",
  lmstudio: "lmstudio",
  qwen: "qwen",
  dashscope: "qwen",
  alibaba: "qwen",
  bailian: "qwen",
  meta: "meta",
  llama: "meta",
  tencent: "hunyuan",
  hunyuan: "hunyuan",
  baidu: "wenxin",
  wenxin: "wenxin",
  qianfan: "wenxin",
  yi: "yi",
  "01ai": "yi",
  lingyiwanwu: "yi",
  cohere: "cohere",
  perplexity: "perplexity",
  baichuan: "baichuan",
};

/** Model-id prefixes/patterns → vendor. Checked in order; first match wins. */
const MODELS: [RegExp, Vendor][] = [
  [/^claude/, "claude"],
  // Codex is part of ChatGPT; the ChatGPT mark is the OpenAI knot, so share the mono `openai.svg`.
  [/^codex/, "openai"],
  [/^(gpt|chatgpt|o[1-9](-|$)|davinci|text-embedding|dall-e|whisper|tts)/, "openai"],
  [/^gemma/, "gemma"],
  [/^(gemini|palm|bison|imagen|veo)/, "gemini"],
  [/^deepseek/, "deepseek"],
  [/^(qwen|qwq|qvq|wan)/, "qwen"],
  [/^(glm|chatglm|cogview|cogvideo)/, "zhipu"],
  [/^(kimi|moonshot)/, "kimi"],
  [/^grok/, "grok"],
  [/^(llama|meta-llama|codellama)/, "meta"],
  [/^(mistral|mixtral|codestral|ministral|pixtral|magistral|devstral)/, "mistral"],
  [/^(minimax|abab|hailuo)/, "minimax"],
  [/^doubao/, "doubao"],
  [/^hunyuan/, "hunyuan"],
  [/^ernie/, "wenxin"],
  [/^yi-/, "yi"],
  [/^(command|c4ai|aya)/, "cohere"],
  [/^(sonar|pplx)/, "perplexity"],
  [/^baichuan/, "baichuan"],
  [/^step-/, "stepfun"],
  [/^nvidia/, "nvidia"],
];

/** Client app names (`owo apps`) and their client ids (as stored in usage rows) → product / vendor logo. */
const APPS: Record<string, Vendor> = {
  codex: "openai",
  "codex-desktop": "openai",
  codex_desktop: "openai",
  claude: "claude",
  claude_code: "claude",
  "claude-desktop": "claude",
  claude_desktop: "claude",
  cursor: "cursor",
  grok: "grok",
  grok_build: "grok",
  copilot: "githubcopilot",
  copilot_app: "githubcopilot",
  opencode: "opencode",
  mcode: "minimax",
  minimax_code: "minimax",
  mmx: "minimax",
  minimax_cli: "minimax",
  zcode: "zhipu",
};

function norm(s: string): string {
  return s.trim().toLowerCase();
}

/** Drop relay-style prefixes: `openrouter/anthropic/claude-x`, `anthropic/claude-x`, `accounts/fireworks/models/x`. */
function stripPrefix(id: string): string {
  const parts = id.split("/");
  return parts[parts.length - 1] ?? id;
}

function vendorOfModel(modelId: string): Vendor | undefined {
  const id = norm(modelId);
  // A `vendor/model` path also names the vendor; use it when the model name alone is unknown.
  const bare = stripPrefix(id);
  for (const [re, v] of MODELS) if (re.test(bare)) return v;
  const segments = id.split("/").slice(0, -1);
  for (const seg of segments) {
    const v = PROVIDERS[seg];
    if (v) return v;
  }
  return undefined;
}

function vendorOfProvider(providerId: string): Vendor | undefined {
  const id = norm(providerId);
  if (PROVIDERS[id]) return PROVIDERS[id];
  // Custom ids like `openai-relay`, `my-anthropic`: match on a known token.
  for (const token of id.split(/[^a-z0-9]+/)) {
    const v = PROVIDERS[token];
    if (v) return v;
  }
  for (const key of Object.keys(PROVIDERS)) if (key.length >= 4 && id.includes(key)) return PROVIDERS[key];
  return undefined;
}

/**
 * Best-effort vendor for a model, using the model id first (relays and
 * aggregators serve many vendors), then the provider id.
 */
export function vendorOf(modelId?: string | null, providerId?: string | null): Vendor | undefined {
  return (modelId ? vendorOfModel(modelId) : undefined) ?? (providerId ? vendorOfProvider(providerId) : undefined);
}

/** Logo for a client app name as listed on the Apps page. */
export function vendorOfApp(app: string): Vendor | undefined {
  return APPS[norm(app)];
}

const TITLE = /<title>.*?<\/title>/;

function Mark({ vendor, className }: { vendor: Vendor; className?: string }) {
  return (
    <span
      className={cn("inline-flex shrink-0 items-center justify-center [&>svg]:block [&>svg]:size-full", className)}
      aria-hidden
      dangerouslySetInnerHTML={{ __html: SVGS[vendor].replace(TITLE, "") }}
    />
  );
}

/**
 * Brand mark for a provider / preset / model id. Falls back to a neutral box
 * when the vendor is unknown. Size it with `className` (e.g. `size-4`).
 */
export function VendorIcon({ id, provider, className }: { id?: string | null; provider?: string | null; className?: string }) {
  const vendor = vendorOf(id, provider);
  if (!vendor) return <Box className={cn("shrink-0 text-muted-foreground", className)} aria-hidden />;
  return <Mark vendor={vendor} className={className} />;
}

/** Product logo for a client app; neutral code icon when unknown. */
export function AppVendorIcon({ app, className }: { app: string; className?: string }) {
  const vendor = vendorOfApp(app);
  if (!vendor) return <Code className={cn("shrink-0", className)} aria-hidden />;
  return <Mark vendor={vendor} className={className} />;
}
