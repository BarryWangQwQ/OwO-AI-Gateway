import { getCurrentWindow } from "@tauri-apps/api/window";
import { createContext, useContext, useEffect, useState, type ReactNode } from "react";

export type Theme = "light" | "dark" | "system";

type ThemeState = { theme: Theme; resolved: "light" | "dark"; setTheme: (theme: Theme) => void };

/** Used until the user picks one in Settings; index.html and the window config start dark to match. */
const DEFAULT_THEME: Theme = "dark";

const Context = createContext<ThemeState>({ theme: DEFAULT_THEME, resolved: "dark", setTheme: () => {} });
const KEY = "owo-theme";
const dark = () => window.matchMedia("(prefers-color-scheme: dark)");

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(() => (localStorage.getItem(KEY) as Theme | null) ?? DEFAULT_THEME);
  const [systemDark, setSystemDark] = useState(() => dark().matches);

  useEffect(() => {
    const media = dark();
    const onChange = () => setSystemDark(media.matches);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

  const resolved = theme === "system" ? (systemDark ? "dark" : "light") : theme;
  useEffect(() => {
    document.documentElement.classList.toggle("dark", resolved === "dark");
    // The native title bar follows too (`null` hands it back to the OS for "system"); outside Tauri this just rejects.
    getCurrentWindow()
      .setTheme(theme === "system" ? null : theme)
      .catch(() => {});
  }, [theme, resolved]);

  // Persisted only on an explicit choice, so people who never touched it follow the default.
  const setTheme = (next: Theme) => {
    localStorage.setItem(KEY, next);
    setThemeState(next);
  };

  return <Context.Provider value={{ theme, resolved, setTheme }}>{children}</Context.Provider>;
}

export const useTheme = () => useContext(Context);
