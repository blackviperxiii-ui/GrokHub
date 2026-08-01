import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const badgeVariants = cva(
  "inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium",
  {
    variants: {
      variant: {
        default:
          "border-[var(--color-border)] bg-[var(--color-elevated)] text-[var(--color-muted)]",
        success:
          "border-[color-mix(in_oklab,var(--color-success)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-success)_12%,transparent)] text-[var(--color-success)]",
        warn:
          "border-[color-mix(in_oklab,var(--color-warn)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-warn)_12%,transparent)] text-[var(--color-warn)]",
        danger:
          "border-[color-mix(in_oklab,var(--color-danger)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-danger)_12%,transparent)] text-[var(--color-danger)]",
        info:
          "border-[color-mix(in_oklab,var(--color-info)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-info)_12%,transparent)] text-[var(--color-info)]",
      },
    },
    defaultVariants: { variant: "default" },
  },
);

export function Badge({
  className,
  variant,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & VariantProps<typeof badgeVariants>) {
  return <div className={cn(badgeVariants({ variant }), className)} {...props} />;
}
