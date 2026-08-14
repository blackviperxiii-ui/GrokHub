# Computer use (Linux)

GrokHub drives the desktop with **silent** grab + CLI injectors. It never opens a screenshot portal.

## What to install (Arch / CachyOS)

```bash
./scripts/provision-computer-use.sh
# or
sudo pacman -S --needed ydotool grim xdotool ffmpeg
```

| Binary | Role |
|--------|------|
| **grim** | Preferred capture on Wayland (full compositor framebuffer, including multi-monitor) |
| **ydotool** | Preferred inject on Wayland (uinput). `/dev/uinput` must be writable |
| **xdotool** | X11 / XWayland inject + `getdisplaygeometry` |
| **ffmpeg** | X11 fallback (`x11grab`) and live MJPEG preview |

Optional: enable `ydotoold.service` if the `ydotool` package ships it.

GrokHub also looks in `$INSTALL/vendor/linux-x64/` before `PATH` if you bundle binaries.

## Geometry (multi-monitor)

Clicks map in **virtual desktop** pixels, not the primary output.

On a 3-monitor KDE span (e.g. 7280×1440), a 1280-wide screenshot is about **1280×253**. That aspect is correct. Mapping still needs a non-zero screen size from, in order:

1. `xdotool getdisplaygeometry`
2. `xdpyinfo` / `xrandr` (no xdotool required)
3. Electron `getAllDisplays()` union

Desktop host → Computer → **Probe** shows session, injector, capture tool, geometry source, uinput, and missing packages.

## Permissions

- KDE often grants `uaccess` on `/dev/uinput` (ACL) without the `input` group.
- Other distros: load `uinput`, add the user to `input`, re-login.
