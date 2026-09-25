import { useMemo } from "react";

import { REFRESH, useQuery } from "@/hooks/use-query";
import { api, type ModelInfo, type Preset, type ProviderInfo } from "@/lib/api";
import { appName, appTitle } from "@/lib/format";

/**
 * Display-name lookups for the ids the usage store and the config hand out. Every function falls back to the id
 * itself, so a model or provider that has since been removed from config.toml still shows as something.
 */

/** A model's display name (`Claude Sonnet 5`), or the id when it has none / is unknown. */
export function modelLabel(id: string | null | undefined, models: ModelInfo[] | undefined): string {
  if (!id) return "—";
  const m = models?.find((x) => x.id === id);
  return m?.displayName?.trim() || id;
}

/** A provider's display name: its own `display_name`, else its preset's name, else the id. */
export function providerLabel(id: string | null | undefined, providers: ProviderInfo[] | undefined, presets?: Preset[]): string {
  if (!id) return "—";
  const p = providers?.find((x) => x.id === id);
  if (p?.displayName?.trim()) return p.displayName;
  const preset = presets?.find((x) => x.id === (p?.preset ?? id));
  return preset?.displayName?.trim() || id;
}

/** The product name for a client id from the usage store (`codex_desktop` → `Codex Desktop`). */
export function appLabel(client: string | null | undefined): string {
  return appTitle(appName(client));
}

export type Names = {
  models: ModelInfo[] | undefined;
  providers: ProviderInfo[] | undefined;
  presets: Preset[] | undefined;
  /**
   * `false` until the three lists have loaded (or failed) once. A page that renders names next to its own data can hold
   * its skeleton on `!ready` so ids don't swap to display names a beat after the content appears.
   */
  ready: boolean;
  model: (id: string | null | undefined) => string;
  provider: (id: string | null | undefined) => string;
  app: (client: string | null | undefined) => string;
  /** `true` when the label differs from the id, i.e. the id is worth showing as a secondary line or tooltip. */
  modelNamed: (id: string | null | undefined) => boolean;
  providerNamed: (id: string | null | undefined) => boolean;
};

/**
 * Loads the model / provider / preset lists (polled at `REFRESH.config`) and returns memoized label functions, so
 * pages that only need to *show* names don't each wire up the same three queries.
 */
export function useNames(): Names {
  const models = useQuery(api.models, [], { refreshInterval: REFRESH.config });
  const providers = useQuery(api.providers, [], { refreshInterval: REFRESH.config });
  const presets = useQuery(api.presets);
  const m = models.data;
  const p = providers.data;
  const s = presets.data;
  const ready = !models.loading && !providers.loading && !presets.loading;
  return useMemo(
    () => ({
      models: m,
      providers: p,
      presets: s,
      ready,
      model: (id) => modelLabel(id, m),
      provider: (id) => providerLabel(id, p, s),
      app: appLabel,
      modelNamed: (id) => !!id && modelLabel(id, m) !== id,
      providerNamed: (id) => !!id && providerLabel(id, p, s) !== id,
    }),
    [m, p, s, ready],
  );
}
