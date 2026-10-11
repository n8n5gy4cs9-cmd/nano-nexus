import { cva, type VariantProps } from "class-variance-authority";
import type { HTMLAttributes } from "react";

import { cn } from "@/lib/cn";

const badgeVariants = cva(
  "inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-caption font-medium",
  {
    variants: {
      tone: {
        neutral: "border-border bg-surface-2 text-muted",
        accent: "border-transparent bg-accent/15 text-accent",
        success: "border-transparent bg-success/15 text-success",
        warning: "border-transparent bg-warning/15 text-warning",
        danger: "border-transparent bg-danger/15 text-danger",
      },
    },
    defaultVariants: { tone: "neutral" },
  },
);

export interface BadgeProps extends HTMLAttributes<HTMLSpanElement>, VariantProps<typeof badgeVariants> {}

export function Badge({ className, tone, ...props }: BadgeProps) {
  return <span className={cn(badgeVariants({ tone }), className)} {...props} />;
}

/** A small colored status dot. */
export function Dot({ tone = "neutral", className }: { tone?: "neutral" | "success" | "warning" | "danger"; className?: string }) {
  const color = { neutral: "bg-faint", success: "bg-success", warning: "bg-warning", danger: "bg-danger" }[tone];
  return <span aria-hidden className={cn("inline-block size-2 rounded-full", color, className)} />;
}
