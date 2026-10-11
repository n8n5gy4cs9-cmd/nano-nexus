import { cn } from "@/lib/cn";

/** Determinate bar; `value` is 0..1, `undefined` shows an indeterminate shimmer. */
export function Progress({ value, className, label }: { value?: number | undefined; className?: string; label: string }) {
  const pct = value === undefined ? undefined : Math.round(Math.min(1, Math.max(0, value)) * 100);
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
      className={cn("h-1.5 w-full overflow-hidden rounded-full bg-surface-3", className)}
    >
      <div
        className={cn("h-full rounded-full bg-[image:var(--nexus-gradient)] transition-[width] duration-[var(--dur-appear)]", pct === undefined && "w-1/3 animate-pulse")}
        style={pct === undefined ? undefined : { width: `${pct}%` }}
      />
    </div>
  );
}

/** Circular progress ring; `value` is 0..1. */
export function ProgressRing({ value, size = 20, label }: { value: number; size?: number; label: string }) {
  const r = (size - 3) / 2;
  const c = 2 * Math.PI * r;
  const v = Math.min(1, Math.max(0, value));
  return (
    <svg role="progressbar" aria-label={label} aria-valuenow={Math.round(v * 100)} width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
      <circle cx={size / 2} cy={size / 2} r={r} fill="none" strokeWidth={3} className="stroke-surface-3" />
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        strokeWidth={3}
        strokeLinecap="round"
        className="stroke-accent"
        strokeDasharray={c}
        strokeDashoffset={c * (1 - v)}
        transform={`rotate(-90 ${size / 2} ${size / 2})`}
      />
    </svg>
  );
}
