import { describe, expect, it } from "vitest";
import type { Capabilities } from "./api";
import { views } from "./Palette";

const DESKTOP: Capabilities = { desktopControl: true, pulse: true, localAi: true, tray: true, webAccess: true, plugins: true, library: true, code: true };

describe("Code tab in the nav", () => {
  it("sits right after Chat on a desktop", () => {
    const tabs = views(DESKTOP);
    expect(tabs.slice(0, 3)).toEqual(["home", "chat", "code"]);
  });

  it("stays off phones and the web app", () => {
    expect(views({ ...DESKTOP, code: false })).not.toContain("code");
    expect(views({ ...DESKTOP, code: undefined })).not.toContain("code");
    expect(views({ ...DESKTOP, remote: true })).not.toContain("code");
  });
});
