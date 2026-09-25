import { useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { Plus, X } from "@/components/icons";
import { Button } from "@/components/ui/button";
import { FieldDescription } from "@/components/ui/field";
import { Textarea } from "@/components/ui/textarea";

type Mode = "simple" | "bulk";

/** localStorage key for the last used mode, so people who prefer the textarea keep it. */
const MODE_KEY = "owo-model-list-mode";

const readMode = (): Mode => (localStorage.getItem(MODE_KEY) === "bulk" ? "bulk" : "simple");

/** Newline- or comma-separated text → trimmed, non-empty, de-duplicated ids (case-sensitive). */
export function parseModelIds(text: string): string[] {
  return [...new Set(text.split(/[\n,]/).map((m) => m.trim()).filter(Boolean))];
}

const sameIds = (a: string[], b: string[]) => a.length === b.length && a.every((id, i) => id === b[i]);

type Props = {
  /** Id of the focusable control (the "add" input or the textarea), for a `<label htmlFor>`. */
  id?: string;
  value: string[];
  onChange: (ids: string[]) => void;
  /** Rendered on the left of the mode toggle; usually a `FieldLabel`. */
  label?: ReactNode;
  /** Textarea placeholder, one example id per line; the "add" input shows its first line. */
  placeholder?: string;
  /** Description under the list in simple mode. */
  hint?: string;
  /** Description under the textarea in bulk mode; defaults to a generic "one per line" note. */
  bulkHint?: string;
};

/**
 * Editor for a list of model ids. Simple mode (default) adds ids one at a time; bulk mode is a
 * plain textarea, one id per line (commas work too). Both edit the same `value` array; the mode is
 * remembered across sessions. Renders a fragment so it slots into a `Field` like a label + control +
 * description trio.
 */
export function ModelIdList({ id, value, onChange, label, placeholder, hint, bulkHint }: Props) {
  const { t } = useTranslation();
  const [mode, setMode] = useState<Mode>(readMode);
  const [pending, setPending] = useState("");
  // Raw textarea text, kept so commas / blank lines survive re-renders. It is only shown while it
  // still parses to `value`; once the array changes from the outside, the canonical join takes over.
  const [text, setText] = useState(() => value.join("\n"));
  const inputRef = useRef<HTMLInputElement>(null);

  const switchMode = (m: Mode) => {
    setMode(m);
    localStorage.setItem(MODE_KEY, m);
  };

  const commit = () => {
    const ids = parseModelIds(pending).filter((m) => !value.includes(m));
    if (ids.length) onChange([...value, ...ids]);
    setPending("");
  };
  const add = () => {
    commit();
    inputRef.current?.focus();
  };

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (e.key === "Enter" || e.key === ",") {
      e.preventDefault();
      add();
    } else if (e.key === "Backspace" && !pending && value.length) {
      onChange(value.slice(0, -1));
    }
  };

  const shownText = sameIds(parseModelIds(text), value) ? text : value.join("\n");

  return (
    <>
      <div className="flex items-center justify-between gap-2">
        {label ?? <span />}
        <Button type="button" variant="ghost" size="xs" className="-my-1 text-muted-foreground" onClick={() => switchMode(mode === "simple" ? "bulk" : "simple")}>
          {mode === "simple" ? t("modelList.bulk") : t("modelList.simple")}
        </Button>
      </div>
      {mode === "simple" ? (
        // Tag input: ids are pills inside one input-styled box, typed into the trailing inline input.
        <div
          className="flex max-h-40 min-h-8 w-full cursor-text flex-wrap items-center gap-1.5 overflow-y-auto rounded-2xl border border-transparent bg-input/50 px-1.5 py-1 transition-[color,box-shadow] duration-200 focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/30"
          onClick={() => inputRef.current?.focus()}
        >
          {value.map((m) => (
            <span key={m} className="inline-flex h-6 max-w-full items-center gap-0.5 rounded-full bg-secondary pr-0.5 pl-2.5 font-mono text-xs text-secondary-foreground">
              <span className="truncate">{m}</span>
              <button
                type="button"
                className="inline-flex size-5 shrink-0 items-center justify-center rounded-full text-muted-foreground outline-none transition-colors hover:bg-foreground/10 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
                aria-label={t("modelList.remove", { id: m })}
                onClick={(e) => {
                  e.stopPropagation();
                  onChange(value.filter((x) => x !== m));
                }}
              >
                <X className="size-3" />
              </button>
            </span>
          ))}
          <input
            ref={inputRef}
            id={id}
            value={pending}
            placeholder={value.length ? t("modelList.more") : t("modelList.placeholder", { example: placeholder?.split("\n")[0] ?? "" })}
            className="h-6 min-w-32 flex-1 bg-transparent px-1 font-mono text-sm outline-none placeholder:font-sans placeholder:text-muted-foreground"
            autoComplete="off"
            spellCheck={false}
            onChange={(e) => setPending(e.target.value)}
            onKeyDown={onKey}
            onBlur={commit}
          />
          {pending.trim() && (
            <Button type="button" variant="ghost" size="icon-xs" className="shrink-0 rounded-full" aria-label={t("modelList.add")} onMouseDown={(e) => e.preventDefault()} onClick={add}>
              <Plus />
            </Button>
          )}
        </div>
      ) : (
        <Textarea
          id={id}
          rows={3}
          className="max-h-48 min-h-0 overflow-y-auto font-mono text-xs"
          value={shownText}
          placeholder={placeholder}
          spellCheck={false}
          onChange={(e) => {
            setText(e.target.value);
            onChange(parseModelIds(e.target.value));
          }}
        />
      )}
      {mode === "simple" ? hint && <FieldDescription>{hint}</FieldDescription> : <FieldDescription>{bulkHint ?? t("modelList.bulkHint")}</FieldDescription>}
    </>
  );
}
