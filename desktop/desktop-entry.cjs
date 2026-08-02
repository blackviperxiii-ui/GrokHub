/**
 * Install / remove GrokHub .desktop entries for app menus + optional autostart.
 * Uses absolute Exec paths so taskbar pins survive after quit (no bare "electron").
 */
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const { execFile, execFileSync } = require("node:child_process");

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

function which(cmd) {
  try {
    return execFileSync("bash", ["-lc", `command -v ${cmd}`], {
      encoding: "utf8",
    }).trim();
  } catch {
    return "";
  }
}

function resolveExec() {
  if (process.env.GROKHUB_EXEC && fs.existsSync(process.env.GROKHUB_EXEC)) {
    return process.env.GROKHUB_EXEC;
  }
  const candidates = [
    which("grokhub"),
    path.join(home(), ".local/bin/grokhub"),
    "/usr/bin/grokhub",
    "/usr/local/bin/grokhub",
  ].filter(Boolean);
  for (const c of candidates) {
    try {
      if (c && fs.existsSync(c)) return c;
    } catch {
      /* next */
    }
  }
  // Last resort: PATH name (still better than electron)
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
      "desktop/icons/grokhub-128.png",
    ]) {
      const p = path.join(r, rel);
      if (fs.existsSync(p)) return p;
    }
  }
  return null;
}

/**
 * Desktop entry body.
 * StartupWMClass must match Chromium/Electron WM_CLASS (second field) = GrokHub.
 * SingleMainWindow + DBusActivatable-style hints help modern GNOME/KDE pins.
 */
function desktopBody(opts = {}) {
  const exec = opts.exec || resolveExec();
  const icon = opts.icon || "grokhub";
  return `[Desktop Entry]
Type=Application
Version=1.5
Name=GrokHub
GenericName=Grok Agent Control Plane
Comment=Grok-native agent desktop — chat, Imagine, connectors, automations, host access
Exec=${exec} %U
TryExec=${exec}
Icon=${icon}
Terminal=false
Categories=Utility;Development;Office;Network;AI;
Keywords=grok;ai;agent;xai;hub;electron;automation;
StartupNotify=true
StartupWMClass=grokhub
SingleMainWindow=true
X-GNOME-SingleWindow=true
X-GNOME-UsesNotifications=true
# Map to this entry when the process is Electron-based
X-AppInstall-Package=grokhub
Actions=NewChat;Settings;Automations;

[Desktop Action NewChat]
Name=New chat
Exec=${exec} --new-chat

[Desktop Action Settings]
Name=Open settings
Exec=${exec} --settings

[Desktop Action Automations]
Name=Automations
Exec=${exec} --automations
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
    const iconName = "grokhub";
    if (iconSrc && fs.existsSync(iconSrc)) {
      for (const size of [16, 22, 24, 32, 48, 64, 128, 256]) {
        try {
          ensureDir(iconsDir(size));
          fs.copyFileSync(iconSrc, path.join(iconsDir(size), "grokhub.png"));
        } catch {
          /* ignore */
        }
      }
      try {
        ensureDir(path.join(home(), ".local/share/pixmaps"));
        fs.copyFileSync(
          iconSrc,
          path.join(home(), ".local/share/pixmaps/grokhub.png"),
        );
      } catch {
        /* ignore */
      }
    }
    const exec = opts.exec || resolveExec();
    fs.writeFileSync(dest, desktopBody({ ...opts, exec, icon: iconName }), {
      mode: 0o644,
    });
    try {
      fs.chmodSync(dest, 0o755);
    } catch {
      /* ignore */
    }
    // Remove stale "Electron" pins that only launch the runtime
    try {
      const bad = path.join(applicationsDir(), "electron.desktop");
      // never delete system electron.desktop; only user-local mis-pins we may have written
    } catch {
      /* ignore */
    }
    refreshCaches();
    return {
      ok: true,
      path: dest,
      exec,
      detail: `App menu entry installed (${exec}) — pin this entry, not Electron`,
    };
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
    const exec = resolveExec();
    fs.writeFileSync(dest, desktopBody({ exec }), { mode: 0o644 });
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
    desktopName: "grokhub.desktop",
    startupWmClass: "grokhub",
  };
}

module.exports = {
  installMenuEntry,
  installAutostart,
  status,
  desktopBody,
  resolveExec,
};
