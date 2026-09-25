import { useEffect, useMemo, useRef } from "react";
import CodeMirror, { type ReactCodeMirrorRef } from "@uiw/react-codemirror";
import { indentLess, indentMore } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { yamlFrontmatter } from "@codemirror/lang-yaml";
import { HighlightStyle, StreamLanguage, indentUnit, syntaxHighlighting } from "@codemirror/language";
import { toml } from "@codemirror/legacy-modes/mode/toml";
import { EditorSelection, Prec, StateEffect, StateField, type Extension } from "@codemirror/state";
import { Decoration, EditorView, keymap, type DecorationSet } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import { cn } from "cn";

import { useTheme } from "@/components/theme";

export type CodeEditorLanguage = "toml" | "markdown";

export type CodeEditorProps = {
  value: string;
  onChange: (value: string) => void;
  /** `toml` (default) or `markdown` (with YAML frontmatter, lines wrapped). */
  language?: CodeEditorLanguage;
  minHeight?: string;
  maxHeight?: string;
  /** Called on Ctrl/Cmd+S. */
  onSave?: () => void;
  /** 1-based line to reveal and mark as problematic (e.g. parsed from an error message). */
  problemLine?: number | null;
  className?: string;
};

// ---------------------------------------------------------------------------
// Theme: chrome uses the shadcn CSS variables so it follows `.dark` on <html>.
// ---------------------------------------------------------------------------

const MONO = "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace)";

const chrome = (dark: boolean) =>
  EditorView.theme(
    {
      "&": { backgroundColor: "transparent", color: "var(--foreground)", fontSize: "13px" },
      "&.cm-focused": { outline: "none" },
      // Scrollbar visuals come from the global ::-webkit-scrollbar rules in index.css;
      // reserving the gutter keeps the text from shifting when the bar appears.
      ".cm-scroller": { fontFamily: MONO, lineHeight: "1.6", scrollbarGutter: "stable" },
      ".cm-content": { padding: "10px 0", caretColor: "var(--foreground)" },
      ".cm-line": { padding: "0 12px" },
      ".cm-gutters": {
        backgroundColor: "transparent",
        color: "var(--muted-foreground)",
        border: "none",
        borderRight: "1px solid var(--border)",
      },
      ".cm-lineNumbers .cm-gutterElement": { padding: "0 10px 0 14px", minWidth: "3em" },
      ".cm-activeLineGutter": { backgroundColor: "var(--muted)", color: "var(--foreground)" },
      ".cm-activeLine": { backgroundColor: "color-mix(in oklab, var(--muted) 55%, transparent)" },
      ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--foreground)" },
      "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
        { backgroundColor: "color-mix(in oklab, var(--primary) 18%, transparent)" },
      ".cm-selectionMatch": { backgroundColor: "color-mix(in oklab, var(--ring) 22%, transparent)" },
      "&.cm-focused .cm-matchingBracket, &.cm-focused .cm-nonmatchingBracket": {
        backgroundColor: "color-mix(in oklab, var(--ring) 28%, transparent)",
        outline: "1px solid color-mix(in oklab, var(--ring) 55%, transparent)",
        borderRadius: "2px",
      },
      ".cm-problemLine": { backgroundColor: "color-mix(in oklab, var(--destructive) 14%, transparent)" },
      ".cm-problemLineGutter": { color: "var(--destructive)", fontWeight: "600" },
      ".cm-panels": { backgroundColor: "var(--popover)", color: "var(--popover-foreground)" },
      ".cm-searchMatch": { backgroundColor: "color-mix(in oklab, var(--ring) 30%, transparent)" },
      ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "color-mix(in oklab, var(--primary) 30%, transparent)" },
    },
    { dark },
  );

// Token colours. The legacy TOML mode emits: property (keys), string, atom
// (section headers, booleans, dates), number, comment and bracket. Markdown adds
// headings, emphasis, links, code and the `#`/`---` marks (meta, processingInstruction),
// which TOML never emits; its YAML frontmatter reuses the property/string colours.
const markdownTokens = (heading: string, link: string, code: string) => [
  { tag: t.heading, color: heading, fontWeight: "600" },
  { tag: t.emphasis, fontStyle: "italic" },
  { tag: t.strong, fontWeight: "600" },
  { tag: [t.link, t.url], color: link },
  { tag: t.monospace, color: code },
  { tag: t.quote, color: "var(--muted-foreground)", fontStyle: "italic" },
  { tag: [t.meta, t.processingInstruction, t.contentSeparator], color: "var(--muted-foreground)" },
];

const lightTokens = HighlightStyle.define([
  { tag: t.propertyName, color: "#0550ae" },
  { tag: t.string, color: "#116329" },
  { tag: t.number, color: "#953800" },
  { tag: t.atom, color: "#8250df" },
  { tag: t.bool, color: "#8250df" },
  { tag: [t.comment, t.lineComment], color: "var(--muted-foreground)", fontStyle: "italic" },
  { tag: [t.bracket, t.squareBracket], color: "var(--muted-foreground)" },
  ...markdownTokens("#0550ae", "#8250df", "#953800"),
]);

const darkTokens = HighlightStyle.define([
  { tag: t.propertyName, color: "#79c0ff" },
  { tag: t.string, color: "#7ee787" },
  { tag: t.number, color: "#ffa657" },
  { tag: t.atom, color: "#d2a8ff" },
  { tag: t.bool, color: "#d2a8ff" },
  { tag: [t.comment, t.lineComment], color: "var(--muted-foreground)", fontStyle: "italic" },
  { tag: [t.bracket, t.squareBracket], color: "var(--muted-foreground)" },
  ...markdownTokens("#79c0ff", "#d2a8ff", "#ffa657"),
]);

const themeFor = (resolved: "light" | "dark"): Extension => [
  chrome(resolved === "dark"),
  syntaxHighlighting(resolved === "dark" ? darkTokens : lightTokens),
];

// ---------------------------------------------------------------------------
// Problem line marker
// ---------------------------------------------------------------------------

const setProblemLine = StateEffect.define<number | null>();
const problemLineMark = Decoration.line({ class: "cm-problemLine" });

const problemLineField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(marks, tr) {
    for (const e of tr.effects) {
      if (e.is(setProblemLine)) {
        if (e.value === null || e.value < 1 || e.value > tr.state.doc.lines) return Decoration.none;
        const line = tr.state.doc.line(e.value);
        return Decoration.set([problemLineMark.range(line.from)]);
      }
    }
    // Any edit invalidates the marker; the message it came from is stale.
    return tr.docChanged ? Decoration.none : marks;
  },
  provide: (f) => EditorView.decorations.from(f),
});

// ---------------------------------------------------------------------------
// Editing behaviour
// ---------------------------------------------------------------------------

const INDENT = "  ";

const insertSoftTab = (view: EditorView) => {
  if (view.state.selection.ranges.some((r) => !r.empty)) return indentMore(view);
  view.dispatch(view.state.replaceSelection(INDENT), { scrollIntoView: true, userEvent: "input" });
  return true;
};

const LANGUAGES: Record<CodeEditorLanguage, Extension> = {
  toml: StreamLanguage.define(toml),
  markdown: [yamlFrontmatter({ content: markdown() }), EditorView.lineWrapping],
};

const staticExtensions: Extension = [
  indentUnit.of(INDENT),
  problemLineField,
  keymap.of([
    { key: "Tab", run: insertSoftTab, shift: indentLess },
  ]),
];

export function CodeEditor({ value, onChange, language = "toml", minHeight = "360px", maxHeight = "70vh", onSave, problemLine, className }: CodeEditorProps) {
  const { resolved } = useTheme();
  const ref = useRef<ReactCodeMirrorRef>(null);
  const onSaveRef = useRef(onSave);
  onSaveRef.current = onSave;

  const extensions = useMemo<Extension[]>(
    () => [
      LANGUAGES[language],
      staticExtensions,
      themeFor(resolved),
      Prec.highest(
        keymap.of([
          {
            key: "Mod-s",
            preventDefault: true,
            run: () => {
              onSaveRef.current?.();
              return true;
            },
          },
        ]),
      ),
    ],
    [resolved, language],
  );

  useEffect(() => {
    const view = ref.current?.view;
    if (!view) return;
    const line = problemLine ?? null;
    if (line === null || line < 1 || line > view.state.doc.lines) {
      view.dispatch({ effects: setProblemLine.of(null) });
      return;
    }
    const { from } = view.state.doc.line(line);
    view.dispatch({
      effects: [setProblemLine.of(line), EditorView.scrollIntoView(from, { y: "center" })],
      selection: EditorSelection.cursor(from),
    });
  }, [problemLine]);

  return (
    <div
      data-slot="code-editor"
      className={cn(
        "overflow-hidden rounded-2xl border bg-background transition-[color,box-shadow] duration-200",
        "focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/20",
        className,
      )}
    >
      <CodeMirror
        ref={ref}
        value={value}
        onChange={onChange}
        theme="none"
        indentWithTab={false}
        minHeight={minHeight}
        maxHeight={maxHeight}
        extensions={extensions}
        basicSetup={{
          lineNumbers: true,
          highlightActiveLine: true,
          highlightActiveLineGutter: true,
          bracketMatching: true,
          closeBrackets: true,
          foldGutter: false,
          autocompletion: false,
          completionKeymap: false,
          lintKeymap: false,
          foldKeymap: false,
          // Our own HighlightStyle is supplied via `extensions`.
          syntaxHighlighting: false,
          tabSize: 2,
        }}
      />
    </div>
  );
}

export default CodeEditor;
