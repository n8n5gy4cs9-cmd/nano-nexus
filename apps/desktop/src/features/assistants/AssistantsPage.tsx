import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Copy, Download, MoreHorizontal, Pencil, Plus, Search, Trash2, Upload } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";

import type { AssistantCard } from "@/bindings";
import {
  AssistantIcon,
  Badge,
  Button,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  EmptyState,
  IconButton,
  Input,
  Skeleton,
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui";
import { ASSISTANTS_KEY, useAssistants } from "@/hooks/useAssistants";
import { api } from "@/lib/api";
import { cn } from "@/lib/cn";
import { toIpcException } from "@/lib/ipc";
import { useAssistantChoice } from "@/stores/assistant";

import { BuilderSheet } from "./BuilderSheet";
import { CATEGORIES } from "./icons";

const fail = (e: unknown) => void toast.error(toIpcException(e).message);

const NEED_LABEL: Record<string, string> = { read: "Reads files", write: "Edits files", create: "Creates files", move: "Moves files", trash: "Trashes files" };

/** Text for the card's "what it can touch" chips. */
export function permissionChips(needs: string[]): string[] {
  return needs.map((n) => NEED_LABEL[n] ?? n);
}

/** Plain names for the tools an assistant may call (Settings → Tools can turn them off). */
export const TOOL_LABEL: Record<string, string> = {
  ask_assistant: "Consult Planner or Reviewer",
  read_file: "Read files",
  list_dir: "List folders",
  search_files: "Find files",
  grep_text: "Search text",
  scan_folder: "Scan folders",
  file_info: "File info",
  find_symbol: "Find symbols",
  doc_search: "Search documents",
  kb_search: "Search libraries",
  write_file: "Propose new files",
  edit_file: "Propose edits",
  move_file: "Propose moves",
  trash_file: "Propose trashing",
  search_web: "Search the web",
  fetch_url: "Read web pages",
  calculate: "Calculator",
  now: "Date and time",
};

function matches(c: AssistantCard, q: string): boolean {
  const hay = `${c.name} ${c.tagline} ${c.category}`.toLowerCase();
  return q
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((w) => hay.includes(w));
}

function Card({ card, onUse, onEdit, onChanged }: { card: AssistantCard; onUse: () => void; onEdit: () => void; onChanged: () => void }) {
  const user = card.source === "user";
  const toggle = useMutation({ mutationFn: () => api.assistants.setEnabled(card.id, !card.enabled), onSuccess: onChanged, onError: fail });
  const duplicate = useMutation({
    mutationFn: () => api.assistants.duplicate(card.id),
    onSuccess: (d) => {
      toast.success(`Created “${d.card.name}”.`);
      onChanged();
    },
    onError: fail,
  });
  const remove = useMutation({ mutationFn: () => api.assistants.delete(card.id), onSuccess: onChanged, onError: fail });
  const exportToml = async () => {
    try {
      const text = await api.assistants.exportToml(card.id);
      await navigator.clipboard.writeText(text);
      downloadText(`${card.id}.toml`, text);
      toast.success("Copied the .toml to your clipboard and started a download.");
    } catch (e) {
      fail(e);
    }
  };
  const body = (
    <div
      className={cn(
        "flex h-full flex-col gap-3 rounded-card border border-border bg-surface p-4 shadow-1 transition-colors",
        card.available ? "hover:bg-surface-2" : "opacity-60",
        !card.enabled && "opacity-50",
      )}
    >
      <div className="flex items-start gap-3">
        <AssistantIcon icon={card.icon} color={card.color} size={40} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h3 className="truncate text-body font-semibold">{card.name}</h3>
            {user ? <Badge tone="accent">Yours</Badge> : null}
            {!card.enabled ? <Badge>Off</Badge> : null}
          </div>
          <p className="line-clamp-2 text-small text-muted">{card.tagline}</p>
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <IconButton aria-label={`More actions for ${card.name}`} size="sm">
              <MoreHorizontal />
            </IconButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {user ? (
              <DropdownMenuItem onSelect={onEdit}>
                <Pencil className="size-4" /> Edit
              </DropdownMenuItem>
            ) : null}
            <DropdownMenuItem onSelect={() => duplicate.mutate()}>
              <Copy className="size-4" /> Duplicate
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => void exportToml()}>
              <Download className="size-4" /> Export .toml
            </DropdownMenuItem>
            {card.id !== "nexus" ? (
              <DropdownMenuItem onSelect={() => toggle.mutate()}>{card.enabled ? "Turn off" : "Turn on"}</DropdownMenuItem>
            ) : null}
            {user ? (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem onSelect={() => remove.mutate()} className="text-danger">
                  <Trash2 className="size-4" /> Delete
                </DropdownMenuItem>
              </>
            ) : null}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <div className="flex flex-wrap items-center gap-1.5">
        <Badge>{card.category}</Badge>
        {permissionChips(card.needs).map((p) => (
          <Badge key={p} tone="warning">
            {p}
          </Badge>
        ))}
        {card.needs.length === 0 ? <Badge tone="success">No file access</Badge> : null}
      </div>
      {card.tools.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5" aria-label="Tools">
          {card.tools.map((t) => (
            <Badge key={t} tone={t === "search_web" || t === "fetch_url" ? "accent" : "neutral"}>
              {TOOL_LABEL[t] ?? t}
            </Badge>
          ))}
        </div>
      ) : null}
      <div className="mt-auto flex justify-end">
        <Button size="sm" variant="primary" disabled={!card.available || !card.enabled} onClick={onUse}>
          Chat
        </Button>
      </div>
    </div>
  );
  if (card.available) return body;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <div tabIndex={0} aria-label={`${card.name}: ${card.unavailable_reason ?? "not available yet"}`}>
          {body}
        </div>
      </TooltipTrigger>
      <TooltipContent>{card.unavailable_reason ?? "Not available yet"}</TooltipContent>
    </Tooltip>
  );
}

/** Save `text` through the webview's download (the app itself never writes to arbitrary paths). */
export function downloadText(name: string, text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** Assistants gallery (UI-DESIGN §6.2): categories, search, cards, and the builder. */
export function AssistantsPage() {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const { cards, loading } = useAssistants();
  const select = useAssistantChoice((s) => s.select);
  const [category, setCategory] = useState<string>("All");
  const [query, setQuery] = useState("");
  const draft = useAssistantChoice((s) => s.builderDraft);
  const [builder, setBuilder] = useState<{ open: boolean; editId: string | null; initialToml?: string | null }>({ open: draft !== null, editId: null, initialToml: draft });
  useEffect(() => {
    if (draft !== null) useAssistantChoice.getState().setBuilderDraft(null);
  }, [draft]);
  const refresh = () => void qc.invalidateQueries({ queryKey: ASSISTANTS_KEY });

  const importFile = useMutation({
    mutationFn: async () => {
      const picked = await api.attachments.pick();
      const file = picked[0];
      if (!file?.text) return null;
      return api.assistants.importToml(file.text);
    },
    onSuccess: (d) => {
      if (d) {
        toast.success(`Imported “${d.card.name}”.`);
        refresh();
      }
    },
    onError: fail,
  });

  const shown = useMemo(
    () => cards.filter((c) => (category === "All" || c.category === category) && (query.trim() === "" || matches(c, query))),
    [cards, category, query],
  );
  const present = useMemo(() => ["All", ...CATEGORIES.filter((c) => cards.some((x) => x.category === c))], [cards]);

  const use = (id: string) => {
    select(id);
    void navigate("/");
  };

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-4 p-6">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-lead font-semibold">Assistants</h1>
          <p className="text-body text-muted">Specialists for files, code, and writing. Everything runs on your computer.</p>
        </div>
        <div className="flex gap-2">
          <Button size="sm" disabled={importFile.isPending} onClick={() => importFile.mutate()}>
            <Upload /> Import
          </Button>
          <Button size="sm" variant="primary" onClick={() => setBuilder({ open: true, editId: null })}>
            <Plus /> New assistant
          </Button>
        </div>
      </header>
      <div className="flex flex-wrap items-center gap-3">
        <div className="relative w-full max-w-xs">
          <Search className="pointer-events-none absolute top-2 left-2.5 size-4 text-faint" />
          <Input aria-label="Search assistants" placeholder="Search assistants" className="pl-8" value={query} onChange={(e) => setQuery(e.target.value)} />
        </div>
        <div className="flex flex-wrap gap-1" role="tablist" aria-label="Categories">
          {present.map((c) => (
            <Button key={c} size="sm" variant={category === c ? "primary" : "ghost"} role="tab" aria-selected={category === c} onClick={() => setCategory(c)}>
              {c}
            </Button>
          ))}
        </div>
      </div>
      {loading ? (
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: 6 }, (_, i) => (
            <Skeleton key={i} className="h-36" />
          ))}
        </div>
      ) : shown.length === 0 ? (
        <EmptyState title="No assistants match" description="Try another word or category." />
      ) : (
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {shown.map((c) => (
            <Card key={c.id} card={c} onUse={() => use(c.id)} onEdit={() => setBuilder({ open: true, editId: c.id })} onChanged={refresh} />
          ))}
        </div>
      )}
      {builder.open ? (
        <BuilderSheet
          editId={builder.editId}
          initialToml={builder.initialToml ?? null}
          onClose={() => setBuilder({ open: false, editId: null })}
          onSaved={(id) => {
            refresh();
            setBuilder({ open: false, editId: null });
            toast.success("Saved.");
            void id;
          }}
        />
      ) : null}
    </div>
  );
}
