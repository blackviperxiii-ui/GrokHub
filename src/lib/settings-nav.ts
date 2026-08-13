export type SettingsCat = "account" | "devices" | "agent" | "memory" | "app";

export type SettingsSectionIntent = {
  cat: SettingsCat;
  sectionId?: string;
};

const EVENT = "grokhub:settings-section";

let pending: SettingsSectionIntent | null = null;

/** Open a Settings category/section even if Settings is not mounted yet. */
export function openSettingsSection(cat: SettingsCat, sectionId?: string): void {
  pending = { cat, sectionId };
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(EVENT, { detail: pending }));
}

export function takePendingSettingsSection(): SettingsSectionIntent | null {
  const next = pending;
  pending = null;
  return next;
}

export function settingsSectionEventName(): string {
  return EVENT;
}
