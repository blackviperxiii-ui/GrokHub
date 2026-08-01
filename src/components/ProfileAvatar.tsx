import { useState } from "react";
import { cn } from "@/lib/utils";

/** Avatar with automatic initials fallback when URL is missing or fails to load. */
export function ProfileAvatar({
  src,
  name,
  email,
  className,
  size = "md",
}: {
  src?: string | null;
  name?: string | null;
  email?: string | null;
  className?: string;
  size?: "sm" | "md" | "lg";
}) {
  const [failed, setFailed] = useState(false);
  const label = (name || email || "G").trim();
  const initials = label
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((p) => p[0]?.toUpperCase() || "")
    .join("") || "G";

  const dim =
    size === "sm" ? "h-8 w-8 text-xs" : size === "lg" ? "h-12 w-12 text-base" : "h-10 w-10 text-sm";

  if (!src || failed) {
    return (
      <div
        className={cn(
          "grid shrink-0 place-items-center rounded-full bg-[var(--color-elevated)] font-medium text-[var(--color-fg)] ring-1 ring-[var(--color-border)]",
          dim,
          className,
        )}
        title={label}
        aria-label={label}
      >
        {initials}
      </div>
    );
  }

  return (
    <img
      src={src}
      alt=""
      title={label}
      className={cn("shrink-0 rounded-full object-cover ring-1 ring-[var(--color-border)]", dim, className)}
      referrerPolicy="no-referrer"
      onError={() => setFailed(true)}
    />
  );
}
