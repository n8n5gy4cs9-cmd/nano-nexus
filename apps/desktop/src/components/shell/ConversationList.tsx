import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Copy, MoreHorizontal, Pencil, Pin, PinOff, Search, Trash2 } from "lucide-react";
import { useState } from "react";
import { NavLink, useMatch, useNavigate } from "react-router-dom";
import { toast } from "sonner";

import type { ConversationSummary } from "@/bindings";
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  Input,
} from "@/components/ui";
import { CONVERSATIONS_KEY, useConversations, useDebounced } from "@/hooks/useConversations";
import { api } from "@/lib/api";
import { cn } from "@/lib/cn";
import { groupConversations } from "@/lib/group";
import { toIpcException } from "@/lib/ipc";

function Row({ c, onRename, onDelete }: { c: ConversationSummary; onRename: () => void; onDelete: () => void }) {
  const qc = useQueryClient();
  const refresh = () => void qc.invalidateQueries({ queryKey: CONVERSATIONS_KEY });
  const pin = useMutation({ mutationFn: () => api.conversations.update(c.id, { title: null, pinned: !c.pinned, assistant_id: null, archived: null }), onSuccess: refresh });
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(await api.conversations.exportMd(c.id));
      toast.success("Conversation copied as Markdown");
    } catch (e) {
      toast.error(toIpcException(e).message);
    }
  };
  return (
    <li className="group relative">
      <NavLink
        to={`/c/${c.id}`}
        className={({ isActive }) =>
          cn(
            "block rounded-button px-2.5 py-1.5 pr-8 text-body text-muted hover:bg-surface-2 hover:text-text",
            isActive && "bg-surface-3 text-text",
          )
        }
      >
        <span className="block truncate">{c.title}</span>
        {c.snippet ? <span className="block truncate text-caption text-faint">{c.snippet}</span> : null}
      </NavLink>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            aria-label={`Actions for ${c.title}`}
            className="absolute top-1/2 right-1 hidden -translate-y-1/2 rounded-chip p-1 text-muted hover:bg-surface-3 group-focus-within:block group-hover:block"
          >
            <MoreHorizontal className="size-4" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem onSelect={onRename}>
            <Pencil className="size-4" /> Rename
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => pin.mutate()}>
            {c.pinned ? <PinOff className="size-4" /> : <Pin className="size-4" />} {c.pinned ? "Unpin" : "Pin"}
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => void copy()}>
            <Copy className="size-4" /> Copy as Markdown
          </DropdownMenuItem>
          <DropdownMenuItem className="text-danger" onSelect={onDelete}>
            <Trash2 className="size-4" /> Delete
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </li>
  );
}

/** Sidebar list: search box, date groups, and per-conversation actions. */
export function ConversationList() {
  const [search, setSearch] = useState("");
  const debounced = useDebounced(search, 200);
  const { data } = useConversations(debounced);
  const qc = useQueryClient();
  const navigate = useNavigate();
  const active = useMatch("/c/:id");
  const [renaming, setRenaming] = useState<ConversationSummary | null>(null);
  const [title, setTitle] = useState("");
  const [deleting, setDeleting] = useState<ConversationSummary | null>(null);
  const refresh = () => void qc.invalidateQueries({ queryKey: CONVERSATIONS_KEY });

  const rename = useMutation({
    mutationFn: (v: { id: string; title: string }) => api.conversations.update(v.id, { title: v.title, pinned: null, assistant_id: null, archived: null }),
    onSuccess: () => {
      setRenaming(null);
      refresh();
    },
    onError: (e) => toast.error(toIpcException(e).message),
  });
  const remove = useMutation({
    mutationFn: (id: string) => api.conversations.delete(id),
    onSuccess: (_r, id) => {
      setDeleting(null);
      refresh();
      if (active?.params.id === id) void navigate("/");
    },
    onError: (e) => toast.error(toIpcException(e).message),
  });

  const groups = groupConversations(data ?? []);
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="px-2 pb-1">
        <div className="relative">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-faint" />
          <Input aria-label="Search conversations" placeholder="Search chats" className="h-7 pl-8 text-small" value={search} onChange={(e) => setSearch(e.target.value)} />
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {groups.length === 0 ? (
          <p className="px-2 py-3 text-caption text-faint">{debounced ? "No matching conversations." : "Conversations will appear here."}</p>
        ) : (
          groups.map((g) => (
            <section key={g.label} className="mt-2">
              <h3 className="px-2.5 pb-1 text-caption font-medium tracking-wide text-faint uppercase">{g.label}</h3>
              <ul className="flex flex-col gap-0.5">
                {g.items.map((c) => (
                  <Row
                    key={c.id}
                    c={c}
                    onRename={() => {
                      setTitle(c.title);
                      setRenaming(c);
                    }}
                    onDelete={() => setDeleting(c)}
                  />
                ))}
              </ul>
            </section>
          ))
        )}
      </div>

      <Dialog open={renaming !== null} onOpenChange={(o) => !o && setRenaming(null)}>
        <DialogContent>
          <DialogTitle>Rename conversation</DialogTitle>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              if (renaming && title.trim()) rename.mutate({ id: renaming.id, title });
            }}
          >
            <Input autoFocus aria-label="Title" className="mt-3" value={title} onChange={(e) => setTitle(e.target.value)} maxLength={120} />
            <DialogFooter>
              <Button variant="ghost" onClick={() => setRenaming(null)}>
                Cancel
              </Button>
              <Button variant="primary" type="submit" disabled={!title.trim()}>
                Save
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog open={deleting !== null} onOpenChange={(o) => !o && setDeleting(null)}>
        <DialogContent>
          <DialogTitle>Delete this conversation?</DialogTitle>
          <DialogDescription>“{deleting?.title}” and all its messages will be removed. This can’t be undone.</DialogDescription>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setDeleting(null)}>
              Cancel
            </Button>
            <Button variant="danger" onClick={() => deleting && remove.mutate(deleting.id)}>
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
