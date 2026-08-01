import { GROK_MODES } from "@/lib/modes";
import { useGrokHub } from "@/lib/store";
import type { GrokModeId } from "@/lib/types";
import { cn } from "@/lib/utils";
import { UsageMeterPanel } from "../UsageMeter";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";

export function SettingsView() {
  const mode = useGrokHub((s) => s.mode);
  const setMode = useGrokHub((s) => s.setMode);
  const desktop = useGrokHub((s) => s.desktop);
  const setDesktop = useGrokHub((s) => s.setDesktop);
  const setNav = useGrokHub((s) => s.setNav);
  const resetDemo = useGrokHub((s) => s.resetDemo);

  return (
    <div className="mx-auto max-w-3xl space-y-5">
      <UsageMeterPanel />

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Model modes (baked in)</CardTitle>
          <CardDescription>
            Same control surface as Grok web — Auto, Fast, Expert, Heavy, Build. All on Grok 4.5.
            Units charged per agent turn by mode.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          {GROK_MODES.map((m) => {
            const selected = m.id === mode;
            const cost =
              m.id === "heavy"
                ? "8 units"
                : m.id === "expert"
                  ? "4 units"
                  : m.id === "build"
                    ? "2 units"
                    : m.id === "auto"
                      ? "1.5 units"
                      : "1 unit";
            return (
              <button
                key={m.id}
                type="button"
                onClick={() => setMode(m.id as GrokModeId)}
                className={cn(
                  "flex w-full items-center justify-between gap-3 rounded-[var(--radius-md)] border px-3 py-3 text-left transition-colors",
                  selected
                    ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)]"
                    : "border-[var(--color-border)] hover:border-[var(--color-border-strong)]",
                )}
              >
                <div>
                  <div className="flex items-center gap-2 text-sm font-medium">
                    {m.label}
                    {m.id === "build" && <Badge className="text-[10px]">Beta</Badge>}
                  </div>
                  <div className="text-xs text-[var(--color-muted)]">
                    {m.subtitle} · {cost}/turn
                  </div>
                </div>
                {selected && (
                  <span className="text-xs text-[var(--color-muted)]">Active</span>
                )}
              </button>
            );
          })}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Unsandboxed desktop host</CardTitle>
          <CardDescription>
            GrokHub is designed as a personal agent with full user-session access: shell, files,
            and installed apps. Host CLI burns 0.25 units per command.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-warn)_35%,transparent)] bg-[color-mix(in_oklab,var(--color-warn)_10%,transparent)] px-3 py-2 text-sm text-[var(--color-warn)]">
            Commands run as your Linux user. Treat this like giving an agent your terminal.
          </div>
          <Button onClick={() => setNav("desktop")}>Open Desktop host</Button>
          <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3 font-mono text-xs leading-relaxed text-[var(--color-muted)]">
            Agent chat: prefix with $ to exec
            <br />
            $ uname -a
            <br />
            Desktop tab: CLI · Files · Apps
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Arch Linux desktop shell</CardTitle>
          <CardDescription>
            Electron preferences for Wayland/X11 (CachyOS, Hyprland, KDE, GNOME).
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {(
            [
              ["wayland", "Prefer Wayland", "Ozone platform flags for Hyprland / KDE / GNOME"],
              ["tray", "System tray", "Minimize to tray; click icon to restore"],
              ["launchOnLogin", "Launch on login", "Autostart via ~/.config/autostart"],
              ["startMinimized", "Start minimized", "Boot to tray only"],
            ] as const
          ).map(([key, label, hint]) => (
            <label
              key={key}
              className="flex cursor-pointer items-center justify-between gap-3 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-3"
            >
              <div>
                <div className="text-sm font-medium">{label}</div>
                <div className="text-xs text-[var(--color-muted)]">{hint}</div>
              </div>
              <input
                type="checkbox"
                className="h-4 w-4 accent-[var(--color-fg)]"
                checked={desktop[key]}
                onChange={(e) => setDesktop({ [key]: e.target.checked })}
              />
            </label>
          ))}

          <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3 font-mono text-xs leading-relaxed text-[var(--color-muted)]">
            <div className="mb-1 text-[10px] uppercase tracking-wide text-[var(--color-subtle)]">
              Arch install
            </div>
            sudo pacman -S electron nodejs npm
            <br />
            npm run desktop:dev
            <br />
            # or: cd packaging && makepkg -si
          </div>
        </CardContent>
      </Card>

      <Button variant="secondary" onClick={resetDemo}>
        Reset demo data
      </Button>
    </div>
  );
}
