import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, File, FileCode, FolderOpen, Folder, FolderPlus } from "lucide-react";
import { useState } from "react";

import type { DirEntryInfo } from "@/bindings";
import { Button, Switch } from "@/components/ui";
import { api } from "@/lib/api";
import { cn } from "@/lib/cn";

import { languageFor } from "./languages";

const IGNORED = new Set([".git", "node_modules", "target", "dist", "build", ".next", "__pycache__", ".venv", ".DS_Store"]);

/** Hide dotfolders and the usual build output (a simple stand-in for .gitignore). */
export function isIgnored(name: string): boolean {
  return IGNORED.has(name) || (name.startsWith(".") && name !== ".gitignore" && name !== ".env.example");
}

function Dir({
  entry,
  depth,
  hideIgnored,
  onOpen,
  active,
}: {
  entry: Pick<DirEntryInfo, "name" | "path">;
  depth: number;
  hideIgnored: boolean;
  onOpen: (path: string) => void;
  active: string | null;
}) {
  const [open, setOpen] = useState(depth === 0);
  const list = useQuery({
    queryKey: ["editor-dir", entry.path],
    queryFn: () => api.fs.listDir(entry.path),
    enabled: open,
    retry: false,
  });
  const entries = (list.data ?? []).filter((e) => !(hideIgnored && isIgnored(e.name)));
  return (
    <li>
      <button
        type="button"
        className="flex h-6 w-full items-center gap-1 rounded-chip pr-2 text-left text-small text-muted hover:bg-surface-2 hover:text-text"
        style={{ paddingLeft: 6 + depth * 12 }}
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
      >
        <ChevronRight className={cn("size-3 shrink-0 transition-transform", open && "rotate-90")} />
        {open ? <FolderOpen className="size-3.5 shrink-0" /> : <Folder className="size-3.5 shrink-0" />}
        <span className="truncate">{entry.name}</span>
      </button>
      {open ? (
        list.isError ? (
          <p className="px-6 py-1 text-caption text-danger">Cannot open this folder.</p>
        ) : (
          <ul>
            {entries.map((e) =>
              e.is_dir ? (
                <Dir key={e.path} entry={e} depth={depth + 1} hideIgnored={hideIgnored} onOpen={onOpen} active={active} />
              ) : (
                <li key={e.path}>
                  <button
                    type="button"
                    onClick={() => onOpen(e.path)}
                    className={cn(
                      "flex h-6 w-full items-center gap-1.5 rounded-chip pr-2 text-left text-small text-muted hover:bg-surface-2 hover:text-text",
                      active === e.path && "bg-surface-3 text-text",
                    )}
                    style={{ paddingLeft: 22 + depth * 12 }}
                    title={e.path}
                  >
                    {languageFor(e.name).id === "text" ? <File className="size-3.5 shrink-0" /> : <FileCode className="size-3.5 shrink-0" />}
                    <span className="truncate">{e.name}</span>
                  </button>
                </li>
              ),
            )}
          </ul>
        )
      ) : null}
    </li>
  );
}

/** Granted folders, lazily listed. */
export function FileTree({
  onOpen,
  active,
  hideIgnored,
  onHideIgnored,
}: {
  onOpen: (path: string) => void;
  active: string | null;
  hideIgnored: boolean;
  onHideIgnored: (v: boolean) => void;
}) {
  const qc = useQueryClient();
  const grants = useQuery({ queryKey: ["grants"], queryFn: api.fs.grants });
  const addFolder = async () => {
    const picked = await api.fs.pickFolder();
    if (!picked) return;
    const covered = (grants.data ?? []).some((g) => picked === g.root || picked.startsWith(g.root + "/") || picked.startsWith(g.root + "\\"));
    // Picking a folder is an explicit act: it gets a read grant. Writing needs the separate button in the editor.
    if (!covered) await api.fs.grantAdd(picked, "read", true);
    await qc.invalidateQueries({ queryKey: ["grants"] });
  };
  const roots = (grants.data ?? []).filter((g) => g.level !== undefined);
  return (
    <aside aria-label="Files" className="flex w-60 shrink-0 flex-col border-r border-border bg-surface">
      <div className="flex items-center justify-between gap-2 border-b border-border p-2">
        <Button size="sm" variant="secondary" onClick={() => void addFolder()}>
          <FolderPlus />
          Open folder
        </Button>
        <label className="flex items-center gap-1.5 text-caption text-muted">
          Hide build files
          <Switch checked={hideIgnored} onCheckedChange={onHideIgnored} aria-label="Hide ignored files" />
        </label>
      </div>
      <ul className="min-h-0 flex-1 overflow-auto p-1">
        {roots.length === 0 ? (
          <li className="p-3 text-small text-muted">Open a folder to start. NanoNexus only shows folders you allow.</li>
        ) : (
          roots.map((g) => (
            <Dir key={g.id} entry={{ name: g.label ?? g.root.split(/[\\/]/).pop() ?? g.root, path: g.root }} depth={0} hideIgnored={hideIgnored} onOpen={onOpen} active={active} />
          ))
        )}
      </ul>
    </aside>
  );
}
