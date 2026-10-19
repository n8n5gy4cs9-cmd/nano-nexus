import { Pause, Play, X } from "lucide-react";

import { Button } from "@/components/ui";

import { clock } from "./types";

export const SPEEDS = [1, 4, 16, 60] as const;

interface Props {
  from: number;
  to: number;
  value: number;
  playing: boolean;
  speed: number;
  onChange: (t: number) => void;
  onPlaying: (p: boolean) => void;
  onSpeed: (s: number) => void;
  onExit: () => void;
}

/** The scrubber over the last few hours (REPLAY mode). */
export function ReplayBar({ from, to, value, playing, speed, onChange, onPlaying, onSpeed, onExit }: Props) {
  const ago = Math.max(0, Math.round((to - value) / 60_000));
  return (
    <div className="flex items-center gap-3 border-t border-border bg-surface/80 px-3 py-2 backdrop-blur" role="group" aria-label="Replay controls">
      <Button size="sm" variant="ghost" aria-label={playing ? "Pause replay" : "Play replay"} onClick={() => onPlaying(!playing)}>
        {playing ? <Pause /> : <Play />}
      </Button>
      <span className="w-24 shrink-0 whitespace-nowrap font-mono text-caption text-faint">{clock(from)}</span>
      <input
        type="range"
        aria-label="Replay position"
        className="h-1 min-w-0 flex-1 cursor-pointer accent-[var(--act-running)]"
        min={from}
        max={to}
        step={1000}
        value={Math.min(to, Math.max(from, value))}
        onChange={(e) => onChange(Number(e.target.value))}
      />
      <span className="w-24 shrink-0 whitespace-nowrap text-right font-mono text-caption text-faint">{clock(to)}</span>
      <span className="w-24 shrink-0 whitespace-nowrap text-right font-mono text-caption text-muted">{ago === 0 ? "now" : `${ago} min ago`}</span>
      <div className="flex gap-1" role="group" aria-label="Replay speed">
        {SPEEDS.map((s) => (
          <button
            key={s}
            type="button"
            aria-pressed={speed === s}
            onClick={() => onSpeed(s)}
            className={`rounded-chip px-1.5 py-0.5 font-mono text-caption ${speed === s ? "bg-surface-3 text-text" : "text-faint hover:text-text"}`}
          >
            {s}×
          </button>
        ))}
      </div>
      <Button size="sm" variant="ghost" aria-label="Back to live" onClick={onExit}>
        <X />
      </Button>
    </div>
  );
}
