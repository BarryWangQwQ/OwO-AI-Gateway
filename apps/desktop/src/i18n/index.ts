import i18next from "i18next";
import { initReactI18next } from "react-i18next";

import { compact } from "@/lib/format";

import en from "./locales/en.json";
import ja from "./locales/ja.json";
import zhCN from "./locales/zh-CN.json";

export const LANGUAGES = [
  { code: "en", label: "English" },
  { code: "zh-CN", label: "简体中文" },
  { code: "ja", label: "日本語" },
] as const;

export type Language = (typeof LANGUAGES)[number]["code"];

const STORAGE_KEY = "owo-lang";

const isLanguage = (s: string | null | undefined): s is Language => !!s && LANGUAGES.some((l) => l.code === s);

/** The saved choice, else the OS language (`zh*` → zh-CN, `ja*` → ja), else English. */
export function detectLanguage(): Language {
  const saved = localStorage.getItem(STORAGE_KEY);
  if (isLanguage(saved)) return saved;
  const system = (navigator.language ?? "").toLowerCase();
  if (system.startsWith("zh")) return "zh-CN";
  if (system.startsWith("ja")) return "ja";
  return "en";
}

void i18next.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    "zh-CN": { translation: zhCN },
    ja: { translation: ja },
  },
  lng: detectLanguage(),
  fallbackLng: "en",
  interpolation: { escapeValue: false },
  returnNull: false,
});

/** `{{count, compact}}` → `1.2K`, so big counts stay short where space is tight (ring centers, heatmap tooltips). */
i18next.services.formatter?.add("compact", (value: unknown) => (typeof value === "number" ? compact(value) : String(value)));

const applyToDocument = (lng: string) => {
  document.documentElement.lang = lng;
};
applyToDocument(i18next.language);
i18next.on("languageChanged", applyToDocument);

/** Switches the UI language and remembers it for the next start. */
export function setLanguage(lng: Language) {
  localStorage.setItem(STORAGE_KEY, lng);
  void i18next.changeLanguage(lng);
}

/** The current language as one of the supported codes. */
export function currentLanguage(): Language {
  return isLanguage(i18next.language) ? i18next.language : "en";
}

export default i18next;
