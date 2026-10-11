import { Hexagon } from "lucide-react";

/** New chat: assistant avatar, tagline, and starter cards (2 x 2). */
export function EmptyChat({ name, tagline, starters, onPick }: { name: string; tagline: string; starters: string[]; onPick: (text: string) => void }) {
  return (
    <div className="mx-auto flex h-full w-full max-w-[760px] flex-col items-center justify-center gap-6 px-4">
      <div className="flex flex-col items-center gap-2 text-center">
        <span className="grid size-14 place-items-center rounded-modal bg-[image:var(--hue-violet)] text-accent-fg shadow-2">
          <Hexagon className="size-7" />
        </span>
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
