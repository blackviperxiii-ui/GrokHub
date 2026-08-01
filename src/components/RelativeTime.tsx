import { useEffect, useState } from "react";
import { formatRelative } from "@/lib/utils";

/** Avoids SSR/client time drift hydration mismatches. */
export function RelativeTime({ ts, className }: { ts: number; className?: string }) {
  const [label, setLabel] = useState("—");

  useEffect(() => {
    const update = () => setLabel(formatRelative(ts));
    update();
    const id = window.setInterval(update, 15000);
    return () => window.clearInterval(id);
  }, [ts]);

  return <span className={className}>{label}</span>;
}
