import type { HTMLAttributes, ReactNode } from "react";

import { cn } from "@/lib/cn";

export function Skeleton({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div aria-hidden className={cn("animate-pulse rounded-chip bg-surface-2", className)} {...props} />;
}

export function Kbd({ className, ...props }: HTMLAttributes<HTMLElement>) {
  return (
    <kbd
      className={cn("rounded-chip border border-border bg-surface-2 px-1.5 py-0.5 text-caption text-muted", className)}
      {...props}
    />
  );
}

export function EmptyState({
  icon,
  title,
  description,
  action,
  className,
}: {
  icon?: ReactNode;
  title: string;
  description?: string;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex flex-col items-center justify-center gap-2 p-10 text-center", className)}>
      {icon ? <div className="text-faint [&_svg]:size-8">{icon}</div> : null}
      <h3 className="text-lead font-semibold">{title}</h3>
      {description ? <p className="max-w-sm text-body text-muted">{description}</p> : null}
      {action ? <div className="mt-2">{action}</div> : null}
    </div>
  );
}

export function StatCard({ label, value, hint, className }: { label: string; value: ReactNode; hint?: string; className?: string }) {
  return (
    <div className={cn("rounded-card border border-border bg-surface p-4 shadow-1", className)}>
      <div className="text-small text-muted">{label}</div>
      <div className="tnum mt-1 text-lead font-semibold">{value}</div>
      {hint ? <div className="mt-0.5 text-caption text-faint">{hint}</div> : null}
    </div>
  );
}

export type AssistantHue = "violet" | "teal" | "amber" | "blue" | "green" | "red" | "indigo" | "pink" | "slate";

/** Gradient tile with an icon (assistant avatar). */
export function AssistantAvatar({ hue, children, size = 32 }: { hue: AssistantHue; children: ReactNode; size?: number }) {
  return (
    <div
      aria-hidden
      className="grid shrink-0 place-items-center rounded-card text-accent-fg [&_svg]:size-1/2"
      style={{ width: size, height: size, backgroundImage: `var(--hue-${hue})` }}
    >
      {children}
    </div>
  );
}
