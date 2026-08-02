/**
 * Install / remove GrokHub .desktop entries for app menus + optional autostart.
 */
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const { execFile } = require("node:child_process");

function home() {
  return process.env.HOME || os.homedir();
}

function applicationsDir() {
  return path.join(home(), ".local/share/applications");
}

function autostartDir() {
  return path.join(home(), ".config/autostart");
}

function iconsDir(size) {
  return path.join(home(), `.local/share/icons/hicolor/${size}x${size}/apps`);
}

function resolveExec() {
  // Prefer PATH wrapper, then common install locations
  const candidates = [
    process.env.GROKHUB_EXEC,
    "grokhub",
    path.join(home(), ".local/bin/grokhub"),
    "/usr/bin/grokhub",
  ].filter(Boolean);
  return candidates[0] || "grokhub";
}

function resolveIconPath() {
  const roots = [
    process.env.GROKHUB_HOME,
    path.join(home(), ".local/share/grokhub"),
    "/usr/lib/grokhub",
    process.cwd(),
  ].filter(Boolean);
  for (const r of roots) {
    for (const rel of [
      "packaging/icons/grokhub-128.png",
      "packaging/icons/hicolor/128x128/apps/grokhub.png",
      "desktop/icons/icon.png",
    ]) {
      const p = path.join(r, rel);
      if (fs.existsSync(p)) return p;
    }
  }
  return "grokhub";
}

function desktopBody(opts = {}) {
  const exec = opts.exec || resolveExec();
  const icon = opts.icon || "grokhub";
  return `[Desktop Entry]
Type=Application
Version=1.0
Name=GrokHub
GenericName=Grok Agent Control Plane
Comment=Grok-native agent desktop — chat, Imagine, connectors, automations, host access
Exec=${exec} %U
Icon=${icon}
Terminal=false
Categories=Utility;Development;Office;Network;AI;
Keywords=grok;ai;agent;xai;hub;electron;automation;
StartupNotify=true
StartupWMClass=GrokHub
X-GNOME-UsesNotifications=true
Actions=NewChat;Settings;

[Desktop Action NewChat]
Name=New chat
Exec=${exec} --new-chat

[Desktop Action Settings]
Name=Settings
Exec=${exec} --settings
`;
}

function ensureDir(d) {
  fs.mkdirSync(d, { recursive: true });
}

function refreshCaches() {
  try {
    execFile("update-desktop-database", [applicationsDir()], () => {});
  } catch {
    /* ignore */
  }
  try {
    execFile(
      "gtk-update-icon-cache",
      ["-f", "-t", path.join(home(), ".local/share/icons/hicolor")],
      () => {},
    );
  } catch {
    /* ignore */
  }
}

function installMenuEntry(opts = {}) {
  try {
    ensureDir(applicationsDir());
    const dest = path.join(applicationsDir(), "grokhub.desktop");
    const iconSrc = resolveIconPath();
    let iconName = "grokhub";
    if (iconSrc && iconSrc !== "grokhub" && fs.existsSync(iconSrc)) {
      for (const size of [128, 64, 48, 32]) {
        try {
          ensureDir(iconsDir(size));
          fs.copyFileSync(iconSrc, path.join(iconsDir(size), "grokhub.png"));
        } catch {
          /* ignore */
        }
      }
      // scalable/pixmaps-style fallback
      try {
        ensureDir(path.join(home(), ".local/share/pixmaps"));
        fs.copyFileSync(iconSrc, path.join(home(), ".local/share/pixmaps/grokhub.png"));
      } catch {
        /* ignore */
      }
    }
    fs.writeFileSync(dest, desktopBody({ ...opts, icon: iconName }), { mode: 0o644 });
    try {
      fs.chmodSync(dest, 0o755);
    } catch {
      /* ignore */
    }
    refreshCaches();
    return { ok: true, path: dest, detail: "App menu entry installed" };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "install failed" };
  }
}

function installAutostart(enabled) {
  try {
    ensureDir(autostartDir());
    const dest = path.join(autostartDir(), "grokhub.desktop");
    if (!enabled) {
      try {
        fs.unlinkSync(dest);
      } catch {
        /* ignore */
      }
      return { ok: true, path: dest, detail: "Autostart disabled" };
    }
    fs.writeFileSync(dest, desktopBody({ exec: resolveExec() }), { mode: 0o644 });
    return { ok: true, path: dest, detail: "Autostart enabled" };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "autostart failed" };
  }
}

function status() {
  const menu = path.join(applicationsDir(), "grokhub.desktop");
  const auto = path.join(autostartDir(), "grokhub.desktop");
  return {
    ok: true,
    menuInstalled: fs.existsSync(menu),
    menuPath: menu,
    autostartInstalled: fs.existsSync(auto),
    autostartPath: auto,
    exec: resolveExec(),
  };
}

module.exports = {
  installMenuEntry,
  installAutostart,
  status,
  desktopBody,
};
