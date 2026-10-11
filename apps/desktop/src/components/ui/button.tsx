import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { forwardRef, type ButtonHTMLAttributes } from "react";

import { cn } from "@/lib/cn";

export const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-button font-medium transition-[background,box-shadow,opacity] duration-[var(--dur-hover)] ease-[var(--ease)] disabled:pointer-events-none disabled:opacity-50 [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        primary: "bg-[image:var(--nexus-gradient)] text-accent-fg shadow-1 hover:brightness-110",
        secondary: "border border-border bg-surface-2 text-text hover:bg-surface-3",
        ghost: "text-muted hover:bg-surface-2 hover:text-text",
        danger: "bg-danger text-accent-fg hover:brightness-110",
      },
      size: {
        sm: "h-7 px-2.5 text-small",
        md: "h-8 px-3 text-body",
      },
    },
    defaultVariants: { variant: "secondary", size: "md" },
  },
);

export interface ButtonProps
  extends ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, type, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp
        ref={ref}
        type={asChild ? undefined : (type ?? "button")}
        className={cn(buttonVariants({ variant, size }), className)}
        {...props}
      />
    );
  },
);
Button.displayName = "Button";

export interface IconButtonProps extends Omit<ButtonProps, "children"> {
  /** Required: icon-only buttons must be labelled for assistive tech. */
  "aria-label": string;
  children: React.ReactNode;
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(
  ({ className, variant = "ghost", size, ...props }, ref) => (
    <Button
      ref={ref}
      variant={variant}
      size={size}
      className={cn(size === "sm" ? "w-7 px-0" : "w-8 px-0", className)}
      {...props}
    />
  ),
);
IconButton.displayName = "IconButton";
