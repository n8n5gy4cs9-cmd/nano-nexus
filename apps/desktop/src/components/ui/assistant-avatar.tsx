import { AssistantAvatar } from "./misc";
import { ASSISTANT_ICONS, isColor } from "@/features/assistants/icons";
import { Sparkles } from "lucide-react";

/** Avatar for an assistant card or spec: its icon on its hue (unknown values fall back safely). */
export function AssistantIcon({ icon, color, size = 32 }: { icon: string; color: string; size?: number }) {
  const Icon = ASSISTANT_ICONS[icon] ?? Sparkles;
  return (
    <AssistantAvatar hue={isColor(color) ? color : "violet"} size={size}>
      <Icon />
    </AssistantAvatar>
  );
}
