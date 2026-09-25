import { createContext, useCallback, useContext, useRef, useState, type ReactNode } from "react";

/**
 * Radix keeps a dialog's content mounted while its exit animation plays, and React keeps re-rendering it with the
 * latest props meanwhile. Pages derive both `open` and the body from one piece of state (`open={!!draft}` next to
 * `{draft && <Form />}`), so the render that closes the dialog also empties it: for the length of the fade-out a
 * dialog with the same title and footer but no body is on screen, which reads as a ghost copy of the one being closed.
 *
 * `Dialog`, `AlertDialog` and `Sheet` publish their open state through this context and their Content keeps rendering
 * the children of the last open render for as long as it is closed (that is, while the exit animation is unmounting it).
 */
const OpenStateContext = createContext<boolean | undefined>(undefined);

export const OpenStateProvider = OpenStateContext.Provider;

type OpenProps = { open?: boolean; defaultOpen?: boolean; onOpenChange?: (open: boolean) => void };

/**
 * The resolved open state of a Root that may be controlled (`open`) or uncontrolled (`defaultOpen` + triggers), plus
 * the `onOpenChange` to hand the Root so the uncontrolled case is tracked as well.
 */
export function useOpenState({ open, defaultOpen = false, onOpenChange }: OpenProps): { isOpen: boolean; onOpenChange: (open: boolean) => void } {
  const [inner, setInner] = useState(defaultOpen);
  const controlled = open !== undefined;
  const handleOpenChange = useCallback(
    (next: boolean) => {
      if (!controlled) setInner(next);
      onOpenChange?.(next);
    },
    [controlled, onOpenChange],
  );
  return { isOpen: open ?? inner, onOpenChange: handleOpenChange };
}

/**
 * `children` while the surrounding Root is open, the children of the last open render once it has closed. Content is
 * only mounted while open or exiting, so "closed" here means the exit animation is running. (A `forceMount`ed Content
 * would stay on its last open children for the whole time it is closed; nothing in the app force-mounts.)
 */
export function useChildrenWhileOpen(children: ReactNode): ReactNode {
  const isOpen = useContext(OpenStateContext);
  const last = useRef<{ children: ReactNode } | null>(null);
  if (isOpen !== false) last.current = { children };
  return last.current ? last.current.children : children;
}
