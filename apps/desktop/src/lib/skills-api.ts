import { invoke } from "@tauri-apps/api/core";

/** The skills commands' app names (`codex` covers the CLI and Codex Desktop). */
export type SkillAppId = "codex" | "claude" | "cursor" | "opencode" | "grok" | "zcode" | "copilot" | "mcode" | "claude-desktop";
/** `switch`: reads ~/.agents/skills, turned off through its own setting; `link`: gets a link in its own folder. */
export type SkillSupport = "switch" | "always-on" | "link" | "unsupported";
export type SkillLocation = "agents" | "codex" | "claude" | "cursor" | "opencode" | "grok" | "zcode" | "copilot";

export type SkillApp = {
  id: SkillAppId;
  name: string;
  support: SkillSupport;
  detected: boolean;
  /** The app's own skills folder. */
  dir: string | null;
  /** The file holding its per-skill switch. */
  switch_file: string | null;
  note: string;
};

export type SkillAppStateKind =
  | "on"
  | "off-by-owo"
  | "off-by-user"
  | "on-by-user"
  | "linked"
  | "not-linked"
  | "foreign"
  | "always-on"
  | "in-folder"
  | "unsupported"
  | "error";

export type SkillAppState = {
  app: SkillAppId;
  enabled: boolean;
  can_toggle: boolean;
  state: SkillAppStateKind;
  /** The app also loads the skill through Claude Code's folder (the same files). */
  duplicate: boolean;
  detail?: string;
};

export type SkillSource = { kind: "local" | "zip" | "github" | "adopted" | "authored"; label: string; repo?: string; commit?: string };

/** One file or folder of a skill; links are listed, never followed. */
export type SkillTreeEntry = { path: string; depth: number; dir: boolean; size: number; link: boolean };
export type SkillTree = { path: string; entries: SkillTreeEntry[]; more: number };

/** One change to a skill's files; paths are relative to the skill folder, `/`-separated. */
export type SkillChange =
  | { op: "write"; path: string; content: string }
  | { op: "mkdir"; path: string }
  /** A Markdown file, or a folder with everything in it. */
  | { op: "delete"; path: string }
  /** A Markdown file or an empty folder. */
  | { op: "rename"; from: string; to: string };

/** A Markdown file of a managed skill, for the editor. */
export type SkillFile = {
  name: string;
  /** The skill folder. */
  path: string;
  content: string;
  source: SkillSource["kind"];
  /** `false` when the skill's folder is a link to somewhere else. */
  editable: boolean;
};
export type SkillUpdate = { latest_commit: string; available: boolean; missing: boolean; checked_at: number };

export type Skill = {
  name: string;
  description: string;
  path: string;
  location: SkillLocation;
  managed: boolean;
  source?: SkillSource;
  /** Unix seconds. */
  installed_at?: number;
  updated_at?: number;
  /** The files differ from what OwO AI Gateway installed. */
  modified: boolean;
  update?: SkillUpdate;
  is_link: boolean;
  can_adopt: boolean;
  apps: SkillAppState[];
  warnings: string[];
};

export type SkillListing = { store: string; apps: SkillApp[]; skills: Skill[]; unmanaged: Skill[] };

export type DiscoveredSkill = {
  name: string;
  description: string;
  dir: string;
  /** The source to pass to `install`. */
  install: string;
  installed: boolean;
  update_available: boolean;
  /** A skill of this name is already installed from somewhere else. */
  conflict: boolean;
  problem?: string;
};

export type SkillRepo = { repo: string; builtin: boolean; commit?: string; fetched_at?: number; skills: DiscoveredSkill[]; error?: string };

export type SkillsResult = { ok: boolean; output: string };

export type InstallOptions = {
  /** Apps to turn it on for (the others off where they allow it); `null` keeps the default: on everywhere. */
  apps?: string[] | null;
  skills?: string[];
  all?: boolean;
  force?: boolean;
};

export const skillsApi = {
  list: () => invoke<SkillListing>("skills_list"),
  /** The configured repositories as last discovered (no network). */
  discovered: () => invoke<SkillRepo[]>("skills_discovered"),
  /** Asks GitHub for stale listings (all of them with `refresh`). */
  discover: (refresh: boolean) => invoke<SkillRepo[]>("skills_discover", { refresh }),
  repos: () => invoke<string[]>("skills_repos"),
  install: (source: string, o: InstallOptions = {}) =>
    invoke<SkillsResult>("skills_install", { source, apps: o.apps ?? null, skills: o.skills ?? [], all: o.all ?? false, force: o.force ?? false }),
  /** A Markdown file of the skill (`SKILL.md` without `path`). */
  read: (name: string, path?: string) => invoke<SkillFile>("skills_read", { name, path: path ?? null }),
  /** One save: creates the skill (`create`; its SKILL.md is a `write` in `changes`) or changes it. `apps` applies to a new skill (`null`: on everywhere). */
  apply: (name: string, create: boolean, changes: SkillChange[], apps: string[] | null = null) => invoke<SkillsResult>("skills_apply", { name, create, changes, apps }),
  tree: (name: string) => invoke<SkillTree>("skills_tree", { name }),
  /** Records hand-written skills' files after changes in the file manager (`names` empty: all). */
  sync: (names: string[] = []) => invoke<SkillsResult>("skills_sync", { names }),
  remove: (name: string) => invoke<SkillsResult>("skills_remove", { name }),
  toggle: (name: string, app: string, enabled: boolean) => invoke<SkillsResult>("skills_toggle", { name, app, enabled }),
  adopt: (name: string) => invoke<SkillsResult>("skills_adopt", { name }),
  /** `names` empty: every managed skill; `check` only reports (and refreshes the update hints). */
  update: (names: string[], check = false) => invoke<SkillsResult>("skills_update", { names, check }),
  addRepo: (repo: string) => invoke<SkillsResult>("skills_repo_add", { repo }),
  removeRepo: (repo: string) => invoke<SkillsResult>("skills_repo_remove", { repo }),
  resetRepos: () => invoke<SkillsResult>("skills_repo_reset"),
  /** A native picker; `null` when cancelled. */
  pick: (kind: "folder" | "zip") => invoke<string | null>("skills_pick", { kind }),
  open: (path: string) => invoke<void>("skills_open", { path }),
};
