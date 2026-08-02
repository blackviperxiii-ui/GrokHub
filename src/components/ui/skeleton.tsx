import { cn } from "@/lib/utils";

export function Skeleton({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn(
        "animate-pulse rounded-[var(--radius-sm)] bg-[color-mix(in_oklab,var(--color-fg)_8%,var(--color-elevated))]",
        className,
      )}
      {...props}
    />
  );
}
