import { Badge, Switch } from "@/components/ui";
import type { SettingValue } from "@/lib/api";
import { useAssistants } from "@/hooks/useAssistants";
import { TOOL_LABEL } from "@/features/assistants/AssistantsPage";

import { Section, ToggleRow } from "./fields";

const WEB_TOOLS = ["search_web", "fetch_url"];
const PROPOSE_TOOLS = ["write_file", "edit_file", "move_file", "trash_file"];

/** `tools.disabled` as an assistant id → tool names map; anything else counts as empty. */
export function parseDisabled(value: unknown): Record<string, string[]> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return {};
  const out: Record<string, string[]> = {};
  for (const [id, list] of Object.entries(value)) {
    if (Array.isArray(list)) out[id] = list.filter((t): t is string => typeof t === "string");
  }
  return out;
}

/** The new `tools.disabled` value after turning one tool on or off for one assistant. */
export function toggleTool(disabled: Record<string, string[]>, assistant: string, tool: string, on: boolean): Record<string, string[]> {
  const rest = (disabled[assistant] ?? []).filter((t) => t !== tool);
  const next = on ? rest : [...rest, tool];
  const others = Object.entries(disabled).filter(([id]) => id !== assistant);
  return Object.fromEntries(next.length === 0 ? others : [...others, [assistant, next]]);
}

/** Settings → Tools: web access, and which tools each assistant may use. */
export function ToolsSection({ settings, set }: { settings: Record<string, unknown>; set: (key: string, value: SettingValue) => void }) {
  const { cards } = useAssistants();
  const webOn = settings["tools.web_enabled"] === true;
  const disabled = parseDisabled(settings["tools.disabled"]);
  const withTools = cards.filter((c) => c.tools.length > 0);

  return (
    <Section title="Tools" hint="Tools let an assistant act. Changes that edit, move or trash files are only proposed: you always review and apply them in Changes. An assistant can only use tools it was built with; here you can switch them off.">
      <ToggleRow
        label="Allow web access"
        hint="Lets assistants that have web tools search the web and read pages. Search words and page addresses leave this computer. Off by default; local addresses are always blocked."
        checked={webOn}
        onChange={(v) => set("tools.web_enabled", v)}
      />
      {withTools.length === 0 ? <p className="py-3 text-small text-muted">No assistant uses tools yet.</p> : null}
      {withTools.map((c) => (
        <div key={c.id} className="py-3" role="group" aria-label={`${c.name} tools`}>
          <div className="mb-2 flex items-center gap-2">
            <span className="text-body font-medium">{c.name}</span>
            {!c.enabled ? <Badge>Off</Badge> : null}
          </div>
          <ul className="grid gap-x-6 gap-y-1 sm:grid-cols-2">
            {c.tools.map((t) => {
              const isWeb = WEB_TOOLS.includes(t);
              const on = !(disabled[c.id] ?? []).includes(t);
              const label = TOOL_LABEL[t] ?? t;
              return (
                <li key={t} className="flex items-center justify-between gap-3">
                  <span className="text-small">
                    {label}
                    {isWeb && !webOn ? <span className="text-muted"> (needs web access)</span> : null}
                    {PROPOSE_TOOLS.includes(t) ? <span className="text-muted"> (you approve)</span> : null}
                  </span>
                  <Switch checked={on} onCheckedChange={(v) => set("tools.disabled", toggleTool(disabled, c.id, t, v))} aria-label={`${c.name}: ${label}`} />
                </li>
              );
            })}
          </ul>
        </div>
      ))}
    </Section>
  );
}
