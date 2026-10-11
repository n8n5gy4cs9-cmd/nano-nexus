import { AssistantIcon } from "@/components/ui";

/** New chat: assistant avatar, tagline, and starter cards (2 x 2). */
export function EmptyChat({
  name,
  icon,
  color,
  tagline,
  starters,
  onPick,
}: {
  name: string;
  icon: string;
  color: string;
  tagline: string;
  starters: string[];
  onPick: (text: string) => void;
}) {
  return (
    <div className="mx-auto flex h-full w-full max-w-[760px] flex-col items-center justify-center gap-6 px-4">
      <div className="flex flex-col items-center gap-2 text-center">
        <AssistantIcon icon={icon} color={color} size={56} />
        <h1 className="text-[24px] leading-8 font-semibold">{name}</h1>
        <p className="max-w-md text-body text-muted">{tagline}</p>
      </div>
      <div className="grid w-full gap-2 sm:grid-cols-2">
        {starters.map((s) => (
          <button
            key={s}
            type="button"
            onClick={() => onPick(s)}
            className="rounded-card border border-border bg-surface p-3 text-left text-body text-muted shadow-1 transition-colors hover:bg-surface-2 hover:text-text"
          >
            {s}
          </button>
        ))}
      </div>
    </div>
  );
}
