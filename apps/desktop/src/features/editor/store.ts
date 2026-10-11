import type { EditorState } from "@codemirror/state";
import { create } from "zustand";

import { readStored, writeStored } from "@/lib/storage";

export interface Tab {
  path: string;
  name: string;
  languageId: string;
  /** blake3 of the file as last read or saved: the base hash for the next save. */
  savedHash: string | null;
  dirty: boolean;
  readOnly: boolean;
  /** Why the tab is read-only (shown in the banner). */
  readOnlyReason: string | null;
  /** Modified time on disk when we last looked (unix ms). */
  mtime: number | null;
  /** The file changed on disk while the buffer has unsaved edits. */
  externalChange: { hash: string; text: string } | null;
}

const SESSION_KEY = "nn.editor.session";

/** One CodeMirror state per open file. The view is created once; states are swapped in and out. */
export const editorStates = new Map<string, EditorState>();

interface EditorStore {
  tabs: Tab[];
  active: string | null;
  /** Recently closed paths, newest last (⌘⇧T). */
  closed: string[];
  wrap: boolean;
  hideIgnored: boolean;
  /** Folders for which the save confirmation is skipped this session. */
  alwaysAllow: string[];
  open: (tab: Tab) => void;
  activate: (path: string) => void;
  patch: (path: string, patch: Partial<Tab>) => void;
  close: (path: string) => void;
  reopen: () => string | null;
  setWrap: (wrap: boolean) => void;
  setHideIgnored: (hide: boolean) => void;
  allowFolder: (dir: string) => void;
}

export function dirOf(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i > 0 ? path.slice(0, i) : path;
}

function saveSession(tabs: Tab[], active: string | null) {
  writeStored(SESSION_KEY, JSON.stringify({ paths: tabs.map((t) => t.path), active }));
}

/** Paths to reopen at start (a list in local storage; contents are read again from disk). */
export function readSession(): { paths: string[]; active: string | null } {
  try {
    const raw = readStored(SESSION_KEY);
    const v: unknown = raw ? JSON.parse(raw) : null;
    if (v && typeof v === "object" && "paths" in v && Array.isArray(v.paths)) {
      const active = "active" in v && typeof v.active === "string" ? v.active : null;
      return { paths: v.paths.filter((p): p is string => typeof p === "string"), active };
    }
  } catch {
    /* ignore a corrupt session */
  }
  return { paths: [], active: null };
}

export const useEditor = create<EditorStore>((set, get) => ({
  tabs: [],
  active: null,
  closed: [],
  wrap: false,
  hideIgnored: true,
  alwaysAllow: [],
  open: (tab) => {
    const tabs = get().tabs.some((t) => t.path === tab.path) ? get().tabs : [...get().tabs, tab];
    set({ tabs, active: tab.path });
    saveSession(tabs, tab.path);
  },
  activate: (path) => {
    set({ active: path });
    saveSession(get().tabs, path);
  },
  patch: (path, patch) => set((s) => ({ tabs: s.tabs.map((t) => (t.path === path ? { ...t, ...patch } : t)) })),
  close: (path) => {
    const { tabs, active, closed } = get();
    const idx = tabs.findIndex((t) => t.path === path);
    if (idx < 0) return;
    const next = tabs.filter((t) => t.path !== path);
    const nextActive = active === path ? (next[Math.min(idx, next.length - 1)]?.path ?? null) : active;
    editorStates.delete(path);
    set({ tabs: next, active: nextActive, closed: [...closed.filter((p) => p !== path), path].slice(-10) });
    saveSession(next, nextActive);
  },
  reopen: () => {
    const { closed } = get();
    const path = closed[closed.length - 1] ?? null;
    if (path) set({ closed: closed.slice(0, -1) });
    return path;
  },
  setWrap: (wrap) => set({ wrap }),
  setHideIgnored: (hideIgnored) => set({ hideIgnored }),
  allowFolder: (dir) => set((s) => (s.alwaysAllow.includes(dir) ? s : { alwaysAllow: [...s.alwaysAllow, dir] })),
}));
