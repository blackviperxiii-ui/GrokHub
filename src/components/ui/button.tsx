import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-[var(--radius-sm)] text-sm font-medium transition-[color,background-color,border-color,opacity,transform] duration-[var(--motion-quick)] ease-[var(--ease-smooth-out)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-ring)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--color-bg)] disabled:pointer-events-none disabled:opacity-50 active:scale-[0.98]",
  {
    variants: {
      variant: {
        default:
          "bg-[var(--color-accent)] text-[var(--color-accent-fg)] shadow-[var(--shadow-hairline)] hover:opacity-92",
        secondary:
          "bg-[var(--color-elevated)] text-[var(--color-fg)] border border-[var(--color-border)] hover:border-[var(--color-border-strong)] hover:bg-[color-mix(in_oklab,var(--color-elevated)_88%,var(--color-fg)_4%)]",
        ghost:
          "text-[var(--color-muted)] hover:bg-[var(--color-elevated)] hover:text-[var(--color-fg)]",
        danger:
          "bg-[color-mix(in_oklab,var(--color-danger)_18%,transparent)] text-[var(--color-danger)] border border-[color-mix(in_oklab,var(--color-danger)_35%,transparent)] hover:bg-[color-mix(in_oklab,var(--color-danger)_28%,transparent)]",
      },
      size: {
        default: "h-10 px-4 py-2",
        sm: "h-8 px-3 text-xs",
        lg: "h-11 px-5",
        icon: "h-10 w-10",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, ...props }, ref) => (
    <button
      ref={ref}
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  ),
);
Button.displayName = "Button";
