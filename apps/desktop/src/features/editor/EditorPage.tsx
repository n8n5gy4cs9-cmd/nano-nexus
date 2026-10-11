import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { bracketMatching, foldGutter, foldKeymap, indentOnInput, indentUnit } from "@codemirror/language";
import { getOriginalDoc, unifiedMergeView } from "@codemirror/merge";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import { Annotation, Compartment, EditorState, Transaction, type Extension, type TransactionSpec } from "@codemirror/state";
import {
  EditorView,
  drawSelection,
  dropCursor,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
  rectangularSelection,
} from "@codemirror/view";
import { useQueryClient } from "@tanstack/react-query";
import { Code2, Save, WrapText, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { Button, EmptyState, IconButton } from "@/components/ui";
import { usePlatform } from "@/hooks/usePlatform";
import { api } from "@/lib/api";
import { cn } from "@/lib/cn";
import { toIpcException } from "@/lib/ipc";
import { hasMod } from "@/lib/platform";

import { AiPanel, type AiState } from "./AiPanel";
import { FileTree } from "./FileTree";
import { languageFor } from "./languages";
import { contextAround, extractFence, isSuspicious, matchTrailingNewline } from "./rewrite";
import { dirOf, editorStates, readSession, useEditor, type Tab } from "./store";
import { nexusTheme } from "./theme";
import type { EditorAction, EditorAiRequest } from "./types";

// Compartments are shared by every tab's state; each state holds its own configuration of them.
const langC = new Compartment();
const wrapC = new Compartment();
const roC = new Compartment();
const mergeC = new Compartment();
const indentC = new Compartment();
/** Marks transactions that come from disk or the AI review, not from typing. */
const programmatic = Annotation.define<boolean>();

const IDLE_AI: AiState = { running: false, text: "", error: null, reviewing: false, suspicious: false, needsSelection: false };

const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p;
const readOnlyExt = (ro: boolean): Extension => (ro ? EditorState.readOnly.of(true) : []);

/** Tabs or 2/4-space indentation, guessed from the first indented lines. */
export function detectIndent(text: string): string {
  let tabs = 0;
  const spaces = new Map<number, number>();
  for (const line of text.split("\n", 400)) {
    if (line.startsWith("\t")) tabs++;
    else {
      const m = /^( +)\S/.exec(line);
      const n = m?.[1]?.length ?? 0;
      if (n >= 2 && n <= 8) spaces.set(n, (spaces.get(n) ?? 0) + 1);
    }
  }
  if (tabs > [...spaces.values()].reduce((a, b) => a + b, 0)) return "\t";
  const two = (spaces.get(2) ?? 0) + (spaces.get(6) ?? 0);
  const four = (spaces.get(4) ?? 0) + (spaces.get(8) ?? 0);
  return two > four ? "  " : "    ";
}

/** The editor: file tree | tabs + CodeMirror | AI actions. */
export function EditorPage() {
  const platform = usePlatform();
  const qc = useQueryClient();
  const { tabs, active, wrap, hideIgnored, alwaysAllow } = useEditor();
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  /** The path whose state is currently inside the view. */
  const loadedRef = useRef<string | null>(null);
  const saveRef = useRef<() => void>(() => undefined);
  const reviewingRef = useRef(false);
  const jobRef = useRef<string | null>(null);
  const [cursor, setCursor] = useState("1:1");
  const [hasSelection, setHasSelection] = useState(false);
  const [ai, setAi] = useState<AiState>(IDLE_AI);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const [always, setAlways] = useState(false);
  const [saving, setSaving] = useState(false);

  const activeTab = tabs.find((t) => t.path === active) ?? null;

  const makeState = useCallback((text: string, tab: Tab, wrapOn: boolean): EditorState => {
    return EditorState.create({
      doc: text,
      extensions: [
        lineNumbers(),
        foldGutter(),
        highlightActiveLine(),
        highlightActiveLineGutter(),
        history(),
        drawSelection(),
        dropCursor(),
        EditorState.allowMultipleSelections.of(true),
        indentOnInput(),
        bracketMatching(),
        rectangularSelection(),
        highlightSelectionMatches(),
        keymap.of([
          {
            key: "Mod-s",
            run: () => {
              saveRef.current();
              return true;
            },
          },
          ...defaultKeymap,
          ...searchKeymap,
          ...historyKeymap,
          ...foldKeymap,
          indentWithTab,
        ]),
        nexusTheme,
        langC.of([]),
        wrapC.of(wrapOn ? EditorView.lineWrapping : []),
        roC.of(readOnlyExt(tab.readOnly)),
        mergeC.of([]),
        indentC.of(indentUnit.of(detectIndent(text))),
        EditorView.updateListener.of((u) => {
          if (u.docChanged && !u.transactions.some((t) => t.annotation(programmatic))) {
            const path = loadedRef.current;
            const cur = path ? useEditor.getState().tabs.find((t) => t.path === path) : undefined;
            if (path && cur && !cur.dirty) useEditor.getState().patch(path, { dirty: true });
          }
          if (u.docChanged || u.selectionSet) {
            const sel = u.state.selection.main;
            const line = u.state.doc.lineAt(sel.head);
            setCursor(`${line.number}:${sel.head - line.from + 1}`);
            setHasSelection(!sel.empty);
          }
        }),
      ],
    });
  }, []);

  /** Apply a change to a tab's state, whether or not it is the one on screen. */
  const applyTo = useCallback((path: string, spec: TransactionSpec) => {
    const view = viewRef.current;
    if (view && loadedRef.current === path) {
      view.dispatch(spec);
      return;
    }
    const st = editorStates.get(path);
    if (st) editorStates.set(path, st.update(spec).state);
  }, []);

  const replaceDoc = useCallback(
    (path: string, text: string) => {
      const view = viewRef.current;
      const len = view && loadedRef.current === path ? view.state.doc.length : (editorStates.get(path)?.doc.length ?? 0);
      applyTo(path, {
        changes: { from: 0, to: len, insert: text },
        annotations: [programmatic.of(true), Transaction.addToHistory.of(false)],
      });
    },
    [applyTo],
  );

  const loadLanguage = useCallback(
    (path: string, name: string) => {
      void languageFor(name)
        .load()
        .then((ext) => applyTo(path, { effects: langC.reconfigure(ext) }))
        .catch(() => undefined);
    },
    [applyTo],
  );

  // Create the single EditorView once.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const view = new EditorView({ parent: host, state: EditorState.create({ doc: "", extensions: [nexusTheme, EditorState.readOnly.of(true)] }) });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
      loadedRef.current = null;
    };
  }, []);

  const endReview = useCallback(() => {
    const view = viewRef.current;
    if (view && reviewingRef.current) view.dispatch({ effects: mergeC.reconfigure([]) });
    reviewingRef.current = false;
  }, []);

  // Swap states when the active tab changes.
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const prev = loadedRef.current;
    if (prev && prev !== active) {
      endReview();
      setAi(IDLE_AI);
      editorStates.set(prev, view.state);
    }
    loadedRef.current = active;
    if (!active) {
      view.setState(EditorState.create({ doc: "", extensions: [nexusTheme, EditorState.readOnly.of(true)] }));
      return;
    }
    const st = editorStates.get(active);
    if (st) {
      view.setState(st);
      view.dispatch({ effects: wrapC.reconfigure(useEditor.getState().wrap ? EditorView.lineWrapping : []) });
      view.focus();
    }
  }, [active, endReview]);

  const openFile = useCallback(
    async (path: string) => {
      setError(null);
      if (useEditor.getState().tabs.some((t) => t.path === path)) {
        useEditor.getState().activate(path);
        return;
      }
      try {
        const [file, canWrite, stat] = await Promise.all([
          api.fs.readText(path),
          api.editor.canWrite(path),
          api.fs.stat(path).catch(() => null),
        ]);
        const name = baseName(file.path || path);
        const tab: Tab = {
          path,
          name,
          languageId: languageFor(name).id,
          savedHash: file.hash,
          dirty: false,
          readOnly: !canWrite,
          readOnlyReason: canWrite ? null : "No write access to this folder.",
          mtime: stat?.modified ?? null,
          externalChange: null,
        };
        editorStates.set(path, makeState(file.text, tab, useEditor.getState().wrap));
        useEditor.getState().open(tab);
        loadLanguage(path, name);
      } catch (e) {
        setError(toIpcException(e).message);
      }
    },
    [loadLanguage, makeState],
  );

  // Reopen last session's tabs once.
  useEffect(() => {
    if (useEditor.getState().tabs.length > 0) return;
    const { paths, active: last } = readSession();
    void (async () => {
      for (const p of paths) await openFile(p);
      if (last && useEditor.getState().tabs.some((t) => t.path === last)) useEditor.getState().activate(last);
    })();
  }, [openFile]);

  const checkExternal = useCallback(
    async (path: string) => {
      const tab = useEditor.getState().tabs.find((t) => t.path === path);
      if (!tab) return;
      try {
        const st = await api.fs.stat(path);
        if (st.modified === tab.mtime) return;
        const file = await api.fs.readText(path);
        const store = useEditor.getState();
        if (file.hash === tab.savedHash) {
          store.patch(path, { mtime: st.modified });
        } else if (!tab.dirty) {
          replaceDoc(path, file.text);
          store.patch(path, { savedHash: file.hash, mtime: st.modified, externalChange: null });
        } else {
          store.patch(path, { mtime: st.modified, externalChange: { hash: file.hash, text: file.text } });
        }
      } catch {
        /* the file may be gone or unreadable: leave the buffer alone */
      }
    },
    [replaceDoc],
  );

  // Poll on focus and when switching tabs.
  useEffect(() => {
    if (active) void checkExternal(active);
    const onFocus = () => {
      const p = useEditor.getState().active;
      if (p) void checkExternal(p);
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [active, checkExternal]);

  const doSave = useCallback(
    async (path: string) => {
      const view = viewRef.current;
      const tab = useEditor.getState().tabs.find((t) => t.path === path);
      if (!view || !tab || loadedRef.current !== path) return;
      setSaving(true);
      setError(null);
      try {
        const r = await api.editor.save(path, view.state.doc.toString(), tab.savedHash);
        const st = await api.fs.stat(path).catch(() => null);
        useEditor.getState().patch(path, { savedHash: r.hash, dirty: false, mtime: st?.modified ?? tab.mtime, externalChange: null });
        void qc.invalidateQueries({ queryKey: ["changes"] });
      } catch (e) {
        const ex = toIpcException(e);
        setError(ex.code === "conflict" ? `${ex.message} Reload the file or keep your version, then save again.` : ex.message);
      } finally {
        setSaving(false);
      }
    },
    [qc],
  );

  const requestSave = useCallback(() => {
    const { active: p, tabs: all, alwaysAllow: allowed } = useEditor.getState();
    const tab = all.find((t) => t.path === p);
    if (!p || !tab || tab.readOnly || !tab.dirty) return;
    if (allowed.includes(dirOf(p))) void doSave(p);
    else setConfirm(p);
  }, [doSave]);
  saveRef.current = requestSave;

  const closeTab = useCallback((path: string) => {
    const tab = useEditor.getState().tabs.find((t) => t.path === path);
    if (tab?.dirty && !window.confirm(`Close ${tab.name} without saving?`)) return;
    useEditor.getState().close(path);
  }, []);

  // Editor shortcuts: ⌘O open, ⌘W close tab, ⌘⇧T reopen.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!hasMod(e, platform) || e.altKey) return;
      const k = e.key.toLowerCase();
      if (k === "o" && !e.shiftKey) {
        e.preventDefault();
        void api.fs.pickAnyFile().then((p) => (p ? openFile(p) : undefined));
      } else if (k === "w" && !e.shiftKey) {
        e.preventDefault();
        const p = useEditor.getState().active;
        if (p) closeTab(p);
      } else if (k === "t" && e.shiftKey) {
        e.preventDefault();
        const p = useEditor.getState().reopen();
        if (p) void openFile(p);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [platform, openFile, closeTab]);

  const toggleWrap = () => {
    const next = !wrap;
    useEditor.getState().setWrap(next);
    viewRef.current?.dispatch({ effects: wrapC.reconfigure(next ? EditorView.lineWrapping : []) });
  };

  const grantWrite = async () => {
    if (!activeTab) return;
    try {
      const grants = await api.fs.grants();
      const covering = grants
        .filter((g) => activeTab.path === g.root || activeTab.path.startsWith(g.root + "/") || activeTab.path.startsWith(g.root + "\\"))
        .sort((a, b) => b.root.length - a.root.length)[0];
      if (covering) await api.fs.grantUpdate(covering.id, { level: "read_write" });
      else await api.fs.grantAdd(dirOf(activeTab.path), "read_write", true);
      const ok = await api.editor.canWrite(activeTab.path);
      if (ok) {
        applyTo(activeTab.path, { effects: roC.reconfigure([]) });
        useEditor.getState().patch(activeTab.path, { readOnly: false, readOnlyReason: null });
      } else setError("Write access is still blocked for this location.");
    } catch (e) {
      setError(toIpcException(e).message);
    }
  };

  const runAi = async (action: EditorAction, instructions: string | null) => {
    const view = viewRef.current;
    if (!view || !activeTab) return;
    const sel = view.state.selection.main;
    if (sel.empty) return;
    const docText = view.state.doc.toString();
    const selection = docText.slice(sel.from, sel.to);
    const around = contextAround(docText, sel.from, sel.to);
    const request: EditorAiRequest = {
      action,
      language: activeTab.languageId,
      file_name: activeTab.name,
      selection,
      context_before: around.before,
      context_after: around.after,
      instructions,
    };
    const rewrite = action !== "explain" && action !== "ask";
    const jobId = crypto.randomUUID();
    jobRef.current = jobId;
    setAi({ ...IDLE_AI, running: true });
    let acc = "";
    let failure: string | null = null;
    try {
      await api.editor.ai(jobId, request, (e) => {
        if (e.type === "content") {
          acc += e.delta;
          setAi((s) => ({ ...s, text: acc }));
        } else if (e.type === "error") failure = e.message;
      });
    } catch (e) {
      failure = toIpcException(e).message;
    }
    jobRef.current = null;
    if (failure) {
      setAi({ ...IDLE_AI, text: acc, error: failure });
      return;
    }
    if (!rewrite) {
      setAi({ ...IDLE_AI, text: acc });
      return;
    }
    const fenced = extractFence(acc);
    if (fenced === null) {
      setAi({ ...IDLE_AI, text: acc, error: "The model did not return code. Try again or select less code." });
      return;
    }
    const proposal = matchTrailingNewline(selection, fenced);
    if (proposal === selection) {
      setAi({ ...IDLE_AI, text: "No changes suggested." });
      return;
    }
    if (view.state.sliceDoc(sel.from, sel.to) !== selection || loadedRef.current !== activeTab.path) {
      setAi({ ...IDLE_AI, error: "The code changed while the model was working. Run the action again." });
      return;
    }
    // Per-chunk accept/reject: show the proposal as a unified diff against the document before it.
    view.dispatch({
      changes: { from: sel.from, to: sel.to, insert: proposal },
      effects: mergeC.reconfigure(unifiedMergeView({ original: view.state.doc, mergeControls: true, gutter: true, highlightChanges: true })),
      scrollIntoView: true,
    });
    reviewingRef.current = true;
    setAi({ ...IDLE_AI, reviewing: true, suspicious: isSuspicious(selection, proposal) });
  };

  const stopAi = () => {
    if (jobRef.current) void api.editor.aiCancel(jobRef.current);
  };
  const finishReview = () => {
    endReview();
    setAi(IDLE_AI);
  };
  const rejectAll = () => {
    const view = viewRef.current;
    if (!view) return;
    const original = getOriginalDoc(view.state).toString();
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: original }, effects: mergeC.reconfigure([]) });
    reviewingRef.current = false;
    setAi(IDLE_AI);
  };

  const confirmName = confirm ? baseName(confirm) : "";

  return (
    <div className="flex h-full min-h-0">
      <FileTree
        onOpen={(p) => void openFile(p)}
        active={active}
        hideIgnored={hideIgnored}
        onHideIgnored={(v) => useEditor.getState().setHideIgnored(v)}
      />
      <section className="flex min-w-0 flex-1 flex-col" aria-label="Editor">
        <div role="tablist" className="flex shrink-0 items-center gap-0.5 overflow-x-auto border-b border-border bg-surface px-1">
          {tabs.map((t) => (
            <div
              key={t.path}
              role="tab"
              aria-selected={t.path === active}
              tabIndex={0}
              title={t.path}
              onClick={() => useEditor.getState().activate(t.path)}
              onKeyDown={(e) => {
                if (e.key === "Enter") useEditor.getState().activate(t.path);
              }}
              onAuxClick={(e) => {
                if (e.button === 1) closeTab(t.path);
              }}
              className={cn(
                "group flex h-8 cursor-default items-center gap-1.5 rounded-t-chip px-2.5 text-small",
                t.path === active ? "bg-code text-text" : "text-muted hover:bg-surface-2",
              )}
            >
              <span className="max-w-40 truncate">{t.name}</span>
              {t.dirty ? <span aria-label="Unsaved changes" className="size-1.5 rounded-full bg-warning" /> : null}
              <button
                type="button"
                aria-label={`Close ${t.name}`}
                className="rounded-chip p-0.5 text-faint opacity-0 hover:bg-surface-3 hover:text-text group-hover:opacity-100 focus-visible:opacity-100"
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(t.path);
                }}
              >
                <X className="size-3" />
              </button>
            </div>
          ))}
          <div className="ml-auto flex items-center gap-1 pr-1">
            <IconButton aria-label="Toggle word wrap" size="sm" aria-pressed={wrap} onClick={toggleWrap}>
              <WrapText />
            </IconButton>
            <Button size="sm" disabled={!activeTab?.dirty || activeTab.readOnly || saving} onClick={requestSave}>
              <Save />
              Save
            </Button>
          </div>
        </div>

        {activeTab?.readOnly ? (
          <div role="status" className="flex items-center gap-3 border-b border-border bg-surface-2 px-3 py-1.5 text-small text-warning">
            <span>Read-only: {activeTab.readOnlyReason ?? "this file cannot be edited."}</span>
            <Button size="sm" variant="secondary" onClick={() => void grantWrite()}>
              Grant write access
            </Button>
          </div>
        ) : null}
        {activeTab?.externalChange ? (
          <div role="alert" className="flex items-center gap-3 border-b border-border bg-surface-2 px-3 py-1.5 text-small text-warning">
            <span>File changed on disk.</span>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => {
                const ch = activeTab.externalChange;
                if (!ch) return;
                replaceDoc(activeTab.path, ch.text);
                useEditor.getState().patch(activeTab.path, { savedHash: ch.hash, dirty: false, externalChange: null });
              }}
            >
              Reload
            </Button>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => {
                const ch = activeTab.externalChange;
                useEditor.getState().patch(activeTab.path, { savedHash: ch?.hash ?? activeTab.savedHash, externalChange: null });
              }}
            >
              Keep mine
            </Button>
          </div>
        ) : null}
        {confirm ? (
          <div role="alertdialog" aria-label="Confirm save" className="flex flex-wrap items-center gap-3 border-b border-border bg-surface-2 px-3 py-1.5 text-small">
            <span>Save changes to {confirmName}? You can undo it from Changes.</span>
            <label className="flex items-center gap-1.5 text-muted">
              <input type="checkbox" checked={always} onChange={(e) => setAlways(e.target.checked)} />
              Always allow for this folder (this session)
            </label>
            <Button
              size="sm"
              onClick={() => {
                if (always) useEditor.getState().allowFolder(dirOf(confirm));
                const p = confirm;
                setConfirm(null);
                void doSave(p);
              }}
            >
              Save
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setConfirm(null)}>
              Cancel
            </Button>
          </div>
        ) : null}
        {error ? (
          <div role="alert" className="border-b border-border bg-surface-2 px-3 py-1.5 text-small text-danger">
            {error}
          </div>
        ) : null}

        <div className="relative min-h-0 flex-1">
          <div ref={hostRef} className="absolute inset-0" />
          {!activeTab ? (
            <div className="absolute inset-0 grid place-items-center bg-code">
              <EmptyState icon={<Code2 />} title="Open a file" description="Pick a file from the tree, or press the open shortcut." />
            </div>
          ) : null}
        </div>

        <footer className="flex h-6 shrink-0 items-center gap-4 border-t border-border bg-surface px-3 text-caption text-muted">
          <span>{activeTab ? languageFor(activeTab.name).label : "No file"}</span>
          <span className="tnum">{activeTab ? cursor : ""}</span>
          <span>UTF-8</span>
          {activeTab?.readOnly ? <span className="text-warning">Read-only (no write grant)</span> : null}
          {alwaysAllow.length > 0 ? <span className="ml-auto">Saves skip confirmation in {alwaysAllow.length} folder(s)</span> : null}
        </footer>
      </section>
      <AiPanel
        state={ai}
        hasSelection={hasSelection && !!activeTab}
        readOnly={activeTab?.readOnly ?? true}
        onRun={(a, i) => void runAi(a, i)}
        onStop={stopAi}
        onFinish={finishReview}
        onRejectAll={rejectAll}
      />
    </div>
  );
}
