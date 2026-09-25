import { useCallback, useEffect, useState } from "react";

/** Desktop-local preference: start the gateway when the app opens. Stored like the theme/language prefs; default off. */
const KEY = "owo-autostart-gateway";
const EVENT = "owo-autostart-gateway-change";

export const readAutoStartGateway = (): boolean => localStorage.getItem(KEY) === "1";

/** The preference plus a setter that persists it immediately; every mounted hook instance sees the change. */
export function useAutoStartGateway(): [boolean, (enabled: boolean) => void] {
  const [enabled, setState] = useState(readAutoStartGateway);

  useEffect(() => {
    const sync = () => setState(readAutoStartGateway());
    window.addEventListener(EVENT, sync);
    window.addEventListener("storage", sync);
    return () => {
      window.removeEventListener(EVENT, sync);
      window.removeEventListener("storage", sync);
    };
  }, []);

  const setEnabled = useCallback((next: boolean) => {
    localStorage.setItem(KEY, next ? "1" : "0");
    setState(next);
    window.dispatchEvent(new Event(EVENT));
  }, []);

  return [enabled, setEnabled];
}
