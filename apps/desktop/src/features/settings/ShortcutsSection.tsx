import { usePlatform } from "@/hooks/usePlatform";
import { SHORTCUTS, shortcutLabel } from "@/lib/shortcuts";

import { Kbd } from "@/components/ui";

import { Section } from "./fields";

/** Settings → Shortcuts: every keyboard shortcut, in this computer's own keys. */
export function ShortcutsSection() {
  const platform = usePlatform();
  const places = ["Anywhere", "Chat", "Editor"] as const;
  return (
    <Section title="Keyboard shortcuts" hint="Everything in NanoNexus can be done from the keyboard.">
      {places.map((place) => (
        <div key={place} className="py-2">
          <h3 className="mb-1 text-small font-semibold text-muted">{place}</h3>
          <ul className="flex flex-col gap-1">
            {SHORTCUTS.filter((s) => s.where === place).map((s) => (
              <li key={`${s.keys}-${s.action}`} className="flex items-baseline justify-between gap-4">
                <span className="text-body">{s.action}</span>
                <Kbd>{shortcutLabel(s, platform)}</Kbd>
              </li>
            ))}
          </ul>
        </div>
      ))}
    </Section>
  );
}
