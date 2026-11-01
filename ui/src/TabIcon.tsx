// Small line icons for the view tabs. They carry the tabs on narrow windows,
// where the labels fold away so every tab fits on one row.
import type { View } from "./Palette";

const PATHS: Record<View, string> = {
  chat: "M4 5h16v11H9l-5 4z",
  pulse: "M3 12h4l2-6 4 12 2-6h6",
  journal: "M6 3h11a1 1 0 0 1 1 1v17H7a2 2 0 0 1-2-2V4a1 1 0 0 1 1-1zM5 18a2 2 0 0 1 2-2h11",
  automations: "M13 3 5 14h6l-1 7 8-11h-6z",
  agents: "M9 6h11M9 12h11M9 18h11M4 6h1M4 12h1M4 18h1",
  memory: "M12 4c-4 0-7 1.3-7 3v10c0 1.7 3 3 7 3s7-1.3 7-3V7c0-1.7-3-3-7-3zM5 7c0 1.7 3 3 7 3s7-1.3 7-3M5 12c0 1.7 3 3 7 3s7-1.3 7-3",
  connectors: "M9 7V3M15 7V3M7 7h10v4a5 5 0 0 1-10 0zM12 16v5",
  settings: "M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0M14 4v4M8 10v4M16 16v4",
};

export function TabIcon({ view }: { view: View }) {
  return (
    <svg
      className="tab-icon"
      viewBox="0 0 24 24"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d={PATHS[view]} />
    </svg>
  );
}
