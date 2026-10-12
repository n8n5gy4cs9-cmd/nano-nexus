import { useQuery } from "@tanstack/react-query";
import { Moon, PanelLeft, RotateCcw, Sun, Terminal } from "lucide-react";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";

import {
  AssistantIcon,
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
  Kbd,
} from "@/components/ui";
import { useAssistants } from "@/hooks/useAssistants";
import { useDebounced } from "@/hooks/useConversations";
import { usePlatform } from "@/hooks/usePlatform";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";
import { modLabel } from "@/lib/platform";
import { IS_WEB } from "@/lib/target";
import { resolveTheme, systemPrefersDark } from "@/lib/theme";
import { NAV, SETTINGS_SECTION } from "@/routes";
import { useActivity } from "@/stores/activity";
import { useAssistantChoice } from "@/stores/assistant";
import { useUi } from "@/stores/ui";

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return <CommandGroup heading={title}>{children}</CommandGroup>;
}

/** ⌘K / Ctrl+K: go to a page, start a chat with an assistant, find a conversation, run an action, switch model. */
export function CommandPalette() {
  const open = useUi((s) => s.paletteOpen);
  const setOpen = useUi((s) => s.setPaletteOpen);
  const platform = usePlatform();
  const navigate = useNavigate();
  const { cards } = useAssistants();
  const [text, setText] = useState("");
  const query = useDebounced(text.trim(), 200);

  const models = useQuery({ queryKey: ["models", "installed"], queryFn: api.models.list, enabled: open && !IS_WEB, retry: false });
  const found = useQuery({
    queryKey: ["palette", "conversations", query],
    queryFn: () => api.conversations.list(query, 8),
    enabled: open && query.length >= 2,
    retry: false,
  });

  const close = () => {
    setOpen(false);
    setText("");
  };
  const run = (fn: () => unknown) => () => {
    close();
    void Promise.resolve(fn()).catch((e: unknown) => toast.error(toIpcException(e).message));
  };
  const go = (path: string) => run(() => navigate(path));

  const ui = useUi.getState;
  const dark = resolveTheme(useUi((s) => s.theme), systemPrefersDark()) === "dark";
  const usable = cards.filter((c) => c.enabled && c.available);
  const mod = modLabel(platform);

  return (
    <Dialog open={open} onOpenChange={(o) => (o ? setOpen(true) : close())}>
      <DialogContent className="top-[18%] translate-y-0 p-0" aria-describedby="palette-desc">
        <DialogTitle className="sr-only">Search and commands</DialogTitle>
        <DialogDescription id="palette-desc" className="sr-only">
          Type to filter pages, assistants, conversations and actions, then press Enter.
        </DialogDescription>
        <Command>
          <CommandInput value={text} onValueChange={setText} placeholder="Search chats, go to a page, run an action…" aria-label="Type a command or search" />
          <CommandList>
            <CommandEmpty>Nothing matches.</CommandEmpty>

            {query.length >= 2 && (found.data ?? []).length > 0 ? (
              <Group title="Conversations">
                {(found.data ?? []).map((c) => (
                  <CommandItem key={c.id} forceMount value={`conversation ${c.id}`} onSelect={go(`/c/${c.id}`)}>
                    <span className="truncate font-medium">{c.title}</span>
                    {c.snippet ? <span className="truncate text-small text-muted">{c.snippet}</span> : null}
                  </CommandItem>
                ))}
              </Group>
            ) : null}

            <Group title="Go to">
              {[...NAV, SETTINGS_SECTION].map((s) => (
                <CommandItem key={s.path} value={`go ${s.label} ${s.tagline}`} onSelect={go(s.path)}>
                  <s.icon className="size-4 text-muted" />
                  <span>{s.label}</span>
                  <span className="truncate text-small text-muted">{s.tagline}</span>
                  {s.digit ? <Kbd className="ml-auto">{mod}{s.digit}</Kbd> : null}
                </CommandItem>
              ))}
            </Group>

            {usable.length > 0 ? (
              <Group title="New chat with">
                {usable.map((c) => (
                  <CommandItem
                    key={c.id}
                    value={`new chat ${c.name} ${c.tagline} ${c.category}`}
                    onSelect={run(() => {
                      useAssistantChoice.getState().select(c.id);
                      return navigate("/");
                    })}
                  >
                    <AssistantIcon icon={c.icon} color={c.color} size={24} />
                    <span className="font-medium">{c.name}</span>
                    <span className="truncate text-small text-muted">{c.tagline}</span>
                  </CommandItem>
                ))}
              </Group>
            ) : null}

            {!IS_WEB && (models.data ?? []).length > 0 ? (
              <Group title="Use model">
                {(models.data ?? []).filter((m) => m.file_exists).map((m) => (
                  <CommandItem
                    key={m.id}
                    value={`use model ${m.name} ${m.quant ?? ""}`}
                    onSelect={run(async () => {
                      const r = await api.models.activate(m.id);
                      if (r.warning) toast.warning(r.warning);
                      else toast.success(`Switching to ${m.name}. The runtime is restarting.`);
                    })}
                  >
                    <span>{m.name}</span>
                    {m.active ? <span className="ml-auto text-small text-muted">active</span> : <span className="ml-auto text-small text-muted">{m.quant}</span>}
                  </CommandItem>
                ))}
              </Group>
            ) : null}

            <Group title="Actions">
              <CommandItem value="action toggle theme dark light" onSelect={run(() => ui().setTheme(dark ? "light" : "dark"))}>
                {dark ? <Sun className="size-4 text-muted" /> : <Moon className="size-4 text-muted" />}
                <span>{dark ? "Switch to the light theme" : "Switch to the dark theme"}</span>
              </CommandItem>
              <CommandItem value="action welcome tour onboarding help" onSelect={run(() => ui().setOnboardingOpen(true))}>
                <span>Show the welcome tour</span>
              </CommandItem>
              <CommandItem value="action toggle sidebar" onSelect={run(() => ui().toggleSidebar())}>
                <PanelLeft className="size-4 text-muted" />
                <span>Show or hide the sidebar</span>
                <Kbd className="ml-auto">{mod}\</Kbd>
              </CommandItem>
              <CommandItem value="action activity console log" onSelect={run(() => useActivity.getState().toggle())}>
                <Terminal className="size-4 text-muted" />
                <span>Show or hide the activity console</span>
                <Kbd className="ml-auto">{mod}J</Kbd>
              </CommandItem>
              {IS_WEB ? null : (
                <>
                  <CommandItem value="action scan a folder analyze" onSelect={go("/analyzer")}>
                    <span>Scan a folder…</span>
                  </CommandItem>
                  <CommandItem value="action analyze a file" onSelect={go("/analyzer")}>
                    <span>Analyze a file…</span>
                  </CommandItem>
                  <CommandItem value="action open rename forge" onSelect={go("/rename")}>
                    <span>Open Rename Forge</span>
                  </CommandItem>
                  <CommandItem value="action restart the model runtime server" onSelect={run(() => api.runtime.restart())}>
                    <RotateCcw className="size-4 text-muted" />
                    <span>Restart the model runtime</span>
                  </CommandItem>
                </>
              )}
            </Group>
          </CommandList>
        </Command>
      </DialogContent>
    </Dialog>
  );
}
