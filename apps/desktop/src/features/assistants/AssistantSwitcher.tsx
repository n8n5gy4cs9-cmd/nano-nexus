import { useQueryClient } from "@tanstack/react-query";
import { useNavigate, useMatch } from "react-router-dom";
import { toast } from "sonner";

import { AssistantIcon, Command, CommandEmpty, CommandInput, CommandItem, CommandList, Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui";
import { useAssistants } from "@/hooks/useAssistants";
import { conversationKey } from "@/features/chat/useChatStream";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";
import { useAssistantChoice } from "@/stores/assistant";

/** ⌘/ : pick the assistant for this chat (or the next new chat). */
export function AssistantSwitcher() {
  const open = useAssistantChoice((s) => s.switcherOpen);
  const setOpen = useAssistantChoice((s) => s.setSwitcherOpen);
  const select = useAssistantChoice((s) => s.select);
  const { cards } = useAssistants();
  const qc = useQueryClient();
  const navigate = useNavigate();
  const inChat = useMatch("/c/:conversationId");
  const conversationId = inChat?.params.conversationId;

  const choose = async (id: string) => {
    setOpen(false);
    select(id);
    if (conversationId) {
      try {
        await api.conversations.update(conversationId, { title: null, pinned: null, assistant_id: id, archived: null });
        await qc.invalidateQueries({ queryKey: conversationKey(conversationId) });
      } catch (e) {
        toast.error(toIpcException(e).message);
      }
    } else {
      void navigate("/");
    }
  };

  const usable = cards.filter((c) => c.enabled && c.available);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="top-[20%] translate-y-0 p-0" aria-describedby="switcher-desc">
        <DialogTitle className="sr-only">Choose an assistant</DialogTitle>
        <DialogDescription id="switcher-desc" className="sr-only">
          Type to filter, then press Enter.
        </DialogDescription>
        <Command>
          <CommandInput placeholder="Switch assistant…" aria-label="Search assistants" />
          <CommandList>
            <CommandEmpty>No assistant found.</CommandEmpty>
            {usable.map((c) => (
              <CommandItem key={c.id} value={`${c.name} ${c.tagline} ${c.category}`} onSelect={() => void choose(c.id)}>
                <AssistantIcon icon={c.icon} color={c.color} size={24} />
                <span className="font-medium">{c.name}</span>
                <span className="truncate text-small text-muted">{c.tagline}</span>
              </CommandItem>
            ))}
          </CommandList>
        </Command>
      </DialogContent>
    </Dialog>
  );
}
