import { invoke } from "@tauri-apps/api/core";

/** USD per million tokens. */
export type Price = { input: number; output: number; cache_read?: number | null; cache_write?: number | null };

export type Status = {
  version: string;
  configPath: string;
  configExists: boolean;
  configError: string | null;
  /** The directory `resetConfig` copies the old config.toml into. */
  configBackupsPath: string;
  name: string;
  gatewayRunning: boolean;
  gatewayAddress: string;
  owoBinary: string | null;
};

export type Summary = {
  key: string;
  calls: number;
  failed: number;
  cancelled: number;
  input_tokens: number;
  cached_input_tokens: number;
  cache_creation_input_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  cost_usd: number | null;
  unpriced: number;
};

export type UsageReport = { since: string; rows: Summary[]; total: Summary };
export type GroupBy = "model" | "app" | "provider" | "day";

export type StoredCall = {
  id: number;
  time: string;
  request_id: string;
  client: string | null;
  requested_model: string;
  model: string | null;
  provider: string | null;
  upstream_model: string | null;
  stream: boolean;
  status: "ok" | "error" | "cancelled";
  error_kind: string | null;
  error_message: string | null;
  upstream_status: number | null;
  duration_ms: number;
  first_token_ms: number | null;
  input_tokens: number | null;
  cached_input_tokens: number | null;
  cache_creation_input_tokens: number | null;
  output_tokens: number | null;
  reasoning_tokens: number | null;
  cost_usd: number | null;
  stop_reason: string | null;
};

export type CallQuery = { failedOnly: boolean; model?: string; client?: string; limit: number };

export type ModelInfo = {
  id: string;
  displayName: string;
  provider: string;
  upstreamModel: string;
  aliases: Record<string, string>;
  price: Price | null;
  contextWindow: number | null;
  available: boolean;
  configured: boolean;
};

export type ProviderRaw = {
  preset: string | null;
  adapter: string | null;
  baseUrl: string | null;
  apiKey: string | null;
  inlineKey: boolean;
  auth: string | null;
  models: string[] | null;
  enabled: boolean;
};

export type ProviderInfo = {
  id: string;
  displayName: string;
  adapter: string;
  baseUrl: string;
  preset: string | null;
  enabled: boolean;
  apiKey: string;
  keyStatus: "ok" | "missing" | "none";
  keyMessage: string | null;
  models: string[];
  raw: ProviderRaw | null;
};

export type Preset = { id: string; displayName: string; adapter: string; baseUrl: string; apiKey: string | null };

export type AppInfo = {
  app: string;
  about: string;
  client_id: string;
  connected: boolean;
  hint: string | null;
  takes_model: boolean;
};

export type ActionResult = { ok: boolean; output: string };
export type AppOutcome = ActionResult & { app: string };
export type ClientConfig = { model?: string | null; name?: string | null };
export type General = { name: string; listen: string; clients: Record<string, ClientConfig> };

export type ModelEdit = {
  id: string;
  /** The current id when editing; a different `id` renames the model (apps set to it follow). */
  originalId?: string;
  provider: string;
  displayName?: string;
  upstreamModel?: string;
  price: Price | null;
};

export type ProviderEdit = {
  id: string;
  /** The current id when editing; a different `id` renames the provider (its models follow). */
  originalId?: string;
  preset: string | null;
  adapter: string | null;
  baseUrl: string | null;
  /** `null` keeps the current key untouched. */
  apiKey: string | null;
  keepApiKey: boolean;
  auth: string | null;
  models: string[];
  enabled: boolean;
};

export const api = {
  status: () => invoke<Status>("status"),
  usage: (days: number, by: GroupBy) => invoke<UsageReport>("usage", { days, by }),
  today: () => invoke<Summary>("today"),
  calls: (query: CallQuery) => invoke<StoredCall[]>("calls", { query }),
  call: (id: number) => invoke<StoredCall | null>("call", { id }),
  /** Deletes every recorded call; resolves to the number deleted. */
  clearHistory: () => invoke<number>("clear_history"),
  models: () => invoke<ModelInfo[]>("models"),
  providers: () => invoke<ProviderInfo[]>("providers"),
  presets: () => invoke<Preset[]>("presets"),
  apps: () => invoke<AppInfo[]>("apps"),
  gatewayStart: () => invoke<ActionResult>("gateway_start"),
  gatewayStop: () => invoke<ActionResult>("gateway_stop"),
  gatewayRestart: () => invoke<ActionResult>("gateway_restart"),
  appConnect: (app: string, model: string | null, force = false) => invoke<ActionResult>("app_connect", { app, model, force }),
  appDisconnect: (app: string, force = false) => invoke<ActionResult>("app_disconnect", { app, force }),
  /** `owo disconnect` for every connected app; one outcome per app. */
  disconnectAll: () => invoke<AppOutcome[]>("disconnect_all"),
  configText: () => invoke<{ path: string; text: string }>("config_text"),
  checkConfigText: (text: string) => invoke<string | null>("check_config_text", { text }),
  saveConfigText: (text: string) => invoke<void>("save_config_text", { text }),
  /** Replaces config.toml with the starter config; resolves to the backup's path (null when there was no file). */
  resetConfig: () => invoke<string | null>("reset_config"),
  general: () => invoke<General>("general"),
  saveGeneral: (name: string, listen: string) => invoke<void>("save_general", { name, listen }),
  saveClient: (app: string, model: string | null, name: string | null) => invoke<void>("save_client", { app, model, name }),
  saveModel: (model: ModelEdit) => invoke<void>("save_model", { model }),
  deleteModel: (id: string) => invoke<void>("delete_model", { id }),
  saveProvider: (provider: ProviderEdit) => invoke<void>("save_provider", { provider }),
  deleteProvider: (id: string) => invoke<void>("delete_provider", { id }),
  setKey: (name: string, secret: string) => invoke<string>("set_key", { name, secret }),
};

// ---------------------------------------------------------------- MCP servers (`owo mcp`)

export type McpTransport = "stdio" | "http" | "sse";

/** An `env` / header entry. `value` is null when it is a plain-text secret the UI never sees. */
export type McpKeyValue = { key: string; value: string | null; reference: boolean; missing: string | null; secret: boolean };

export type McpServerView = {
  transport: McpTransport;
  description: string | null;
  command: string | null;
  args: string[];
  cwd: string | null;
  url: string | null;
  env: McpKeyValue[];
  headers: McpKeyValue[];
  summary: string;
};

export type McpEntryState = "synced" | "outdated" | "modified" | "missing" | "pending" | "not_installed" | "unsupported" | "stale" | "off";

export type McpAppState = { app: string; enabled: boolean; state: McpEntryState; unsupported: string | null };
export type McpServer = McpServerView & { name: string; apps: McpAppState[] };

export type McpAppInfo = {
  app: string;
  label: string;
  supported: boolean;
  installed: boolean;
  files: string[];
  transports: McpTransport[];
  cwd: boolean;
  reason: string | null;
};

export type McpList = { apps: McpAppInfo[]; servers: McpServer[] };

/** An MCP server entry found in an app's own config (`owo mcp scan`). */
export type McpFound = {
  app: string;
  file: string;
  name: string;
  managed: boolean;
  relation: "new" | "same" | "different";
  server: McpServerView | null;
  error: string | null;
  dropped: string[];
  secrets: string[];
};

/** `value: null` keeps the current (hidden) value. */
export type McpPair = { key: string; value: string | null };

export type McpEdit = {
  name: string;
  replace: boolean;
  transport: McpTransport;
  description: string | null;
  command: string | null;
  args: string[];
  env: McpPair[];
  cwd: string | null;
  url: string | null;
  headers: McpPair[];
  apps: string[];
  force: boolean;
};

export const mcpApi = {
  list: () => invoke<McpList>("mcp_list"),
  scan: () => invoke<McpFound[]>("mcp_scan"),
  save: (server: McpEdit) => invoke<ActionResult>("mcp_save", { server }),
  remove: (name: string, force = false) => invoke<ActionResult>("mcp_remove", { name, force }),
  toggle: (name: string, app: string, enabled: boolean, force = false) => invoke<ActionResult>("mcp_toggle", { name, app, enabled, force }),
  /** `app` may be `all`: take the server over from every app that has the same one. */
  import: (app: string, names: string[], keyring: boolean, force = false) => invoke<ActionResult>("mcp_import", { app, names, keyring, force }),
  sync: (force = false) => invoke<ActionResult>("mcp_sync", { force }),
};
