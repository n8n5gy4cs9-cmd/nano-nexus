import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";

// Colors come from the design tokens only (CSS variables), so light/dark follow the app theme.
const view = EditorView.theme({
  "&": { color: "var(--text)", backgroundColor: "var(--code-bg)", height: "100%", fontSize: "13px" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "20px" },
  ".cm-content": { caretColor: "var(--accent)" },
  ".cm-cursor": { borderLeftColor: "var(--accent)" },
  ".cm-gutters": { backgroundColor: "var(--code-bg)", color: "var(--text-faint)", border: "none" },
  ".cm-activeLineGutter": { backgroundColor: "var(--surface-2)", color: "var(--text-muted)" },
  ".cm-activeLine": { backgroundColor: "color-mix(in oklch, var(--surface-2) 60%, transparent)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
    backgroundColor: "color-mix(in oklch, var(--accent) 30%, transparent)",
  },
  ".cm-matchingBracket": { backgroundColor: "color-mix(in oklch, var(--accent) 25%, transparent)", outline: "none" },
  ".cm-foldPlaceholder": { backgroundColor: "var(--surface-3)", border: "none", color: "var(--text-muted)" },
  ".cm-panels": { backgroundColor: "var(--surface)", color: "var(--text)", borderColor: "var(--border)" },
  ".cm-panels input, .cm-panels button": { fontFamily: "var(--font-sans)", fontSize: "12px" },
  ".cm-searchMatch": { backgroundColor: "color-mix(in oklch, var(--warning) 35%, transparent)" },
  ".cm-tooltip": { backgroundColor: "var(--surface-2)", border: "1px solid var(--border)", color: "var(--text)" },
  // merge view (AI proposals)
  ".cm-changedLine": { backgroundColor: "color-mix(in oklch, var(--success) 12%, transparent)" },
  ".cm-deletedChunk": { backgroundColor: "color-mix(in oklch, var(--danger) 12%, transparent)" },
  ".cm-insertedLine, .cm-changedText": { backgroundColor: "color-mix(in oklch, var(--success) 25%, transparent)" },
  ".cm-deletedChunk .cm-deletedText": { backgroundColor: "color-mix(in oklch, var(--danger) 30%, transparent)" },
  ".cm-chunkButtons button": {
    cursor: "pointer",
    borderRadius: "var(--radius-chip)",
    border: "1px solid var(--border)",
    padding: "0 6px",
    fontSize: "11px",
  },
  ".cm-chunkButtons button[name=accept]": { backgroundColor: "var(--success)", color: "var(--accent-fg)" },
  ".cm-chunkButtons button[name=reject]": { backgroundColor: "var(--danger)", color: "var(--accent-fg)" },
});

const highlight = HighlightStyle.define([
  { tag: [t.keyword, t.controlKeyword, t.moduleKeyword, t.operatorKeyword], color: "var(--accent)" },
  { tag: [t.string, t.special(t.string), t.regexp], color: "var(--success)" },
  { tag: [t.number, t.bool, t.null, t.atom], color: "var(--warning)" },
  { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], color: "var(--text-faint)", fontStyle: "italic" },
  { tag: [t.typeName, t.className, t.namespace], color: "var(--info)" },
  { tag: [t.function(t.variableName), t.function(t.propertyName), t.macroName], color: "var(--accent-2)" },
  { tag: [t.propertyName, t.attributeName], color: "var(--text)" },
  { tag: [t.heading], color: "var(--accent)", fontWeight: "600" },
  { tag: [t.link, t.url], color: "var(--info)", textDecoration: "underline" },
  { tag: [t.invalid], color: "var(--danger)" },
]);

/** The editor theme (tokens only) plus syntax colors. */
export const nexusTheme = [view, syntaxHighlighting(highlight)];
