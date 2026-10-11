import * as ScrollPrimitive from "@radix-ui/react-scroll-area";
import { forwardRef, type ComponentPropsWithoutRef, type ElementRef } from "react";

import { cn } from "@/lib/cn";

export const ScrollArea = forwardRef<
  ElementRef<typeof ScrollPrimitive.Root>,
  ComponentPropsWithoutRef<typeof ScrollPrimitive.Root>
>(({ className, children, ...props }, ref) => (
  <ScrollPrimitive.Root ref={ref} className={cn("relative overflow-hidden", className)} {...props}>
    <ScrollPrimitive.Viewport className="size-full rounded-[inherit]">{children}</ScrollPrimitive.Viewport>
    <ScrollPrimitive.Scrollbar orientation="vertical" className="flex w-2 touch-none p-0.5 select-none">
      <ScrollPrimitive.Thumb className="relative flex-1 rounded-full bg-border-strong" />
    </ScrollPrimitive.Scrollbar>
  </ScrollPrimitive.Root>
));
ScrollArea.displayName = "ScrollArea";
