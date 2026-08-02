/** Keyboard shortcuts registry for cheatsheet + help */

export type Shortcut = {
  keys: string;
  action: string;
  scope: string;
};

export const SHORTCUTS: Shortcut[] = [
  { keys: "Ctrl/Cmd+K", action: "Command palette", scope: "Global" },
  { keys: "Ctrl/Cmd+N", action: "New chat", scope: "Global" },
  { keys: "Ctrl/Cmd+L", action: "Focus composer", scope: "Global" },
  { keys: "Ctrl/Cmd+F", action: "Find in chat", scope: "Agent" },
  { keys: "Ctrl/Cmd+[ / ]", action: "Previous / next chat", scope: "Global" },
  { keys: "Ctrl/Cmd+1…5", action: "Jump Agent / History / Command / Workboard / Imagine", scope: "Global" },
  { keys: "Ctrl/Cmd+/", action: "Keyboard shortcuts", scope: "Global" },
  { keys: "Enter", action: "Send message", scope: "Composer" },
  { keys: "Shift+Enter", action: "New line", scope: "Composer" },
  { keys: "Esc", action: "Stop generation / close menus", scope: "Global" },
  { keys: "↑ / ↓", action: "Shell history (when draft is $ / empty)", scope: "Composer / Desktop CLI" },
  { keys: "Enter / Esc", action: "Allow / deny host commands", scope: "Host confirm" },
  { keys: "A / S / D", action: "Approve / Stage / Dismiss work item (focused)", scope: "Workboard" },
];
