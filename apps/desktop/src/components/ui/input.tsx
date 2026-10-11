import { forwardRef, useCallback, useLayoutEffect, useRef, type InputHTMLAttributes, type TextareaHTMLAttributes } from "react";

import { cn } from "@/lib/cn";

const field =
  "w-full rounded-button border border-border bg-surface-2 px-3 text-body text-text placeholder:text-faint transition-colors duration-[var(--dur-hover)] focus-visible:border-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50";

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  ({ className, ...props }, ref) => <input ref={ref} className={cn(field, "h-8", className)} {...props} />,
);
Input.displayName = "Input";

export interface TextareaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  /** Grow with content up to this many px (then scroll). */
  maxHeight?: number;
}

/** Auto-growing textarea. */
export const Textarea = forwardRef<HTMLTextAreaElement, TextareaProps>(
  ({ className, maxHeight = 240, onInput, value, ...props }, forwarded) => {
    const inner = useRef<HTMLTextAreaElement | null>(null);
    const fit = useCallback(() => {
      const el = inner.current;
      if (!el) return;
      el.style.height = "auto";
      el.style.height = `${Math.min(el.scrollHeight, maxHeight)}px`;
    }, [maxHeight]);
    useLayoutEffect(fit, [fit, value]);
    return (
      <textarea
        ref={(el) => {
          inner.current = el;
          if (typeof forwarded === "function") forwarded(el);
          else if (forwarded) forwarded.current = el;
        }}
        value={value}
        rows={1}
        onInput={(e) => {
          fit();
          onInput?.(e);
        }}
        className={cn(field, "resize-none py-1.5", className)}
        {...props}
      />
    );
  },
);
Textarea.displayName = "Textarea";
