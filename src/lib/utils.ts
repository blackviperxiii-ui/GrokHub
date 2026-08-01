import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatRelative(ts: number): string {
  if (!Number.isFinite(ts) || ts <= 0) return "—";
  const diff = Date.now() - ts;
  if (diff < 0) {
    const sec = Math.floor(-diff / 1000);
    if (sec < 60) return "in a moment";
    return "soon";
  }
  const sec = Math.floor(diff / 1000);
  if (sec < 10) return "just now";
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  if (day > 60) return "just now"; // guard absurd frozen seeds
  return `${day}d ago`;
}

export function uid(prefix = "id"): string {
  return `${prefix}_${Math.random().toString(36).slice(2, 9)}`;
}
