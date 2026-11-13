"""Render every shipped app icon from brand/app-icon.svg.

One source of truth for the icon the OS shows (taskbar, alt-tab, exe, tray,
installer, Linux app menu, Tauri bundles). Swap the art in brand/ and run:

    python3 scripts/render-brand-icons.py

Needs cairosvg and Pillow (`python3 -m pip install cairosvg pillow`). File
names stay `grokhub.*` on purpose: the .desktop `Icon=`, the installer and
the updater look them up by that name (brand/README.md, internal ids).
Then run scripts/write-grokhub-ico.py so the Windows .ico frames match the
hicolor PNGs byte for byte (tray.rs `windows_ico_matches_linux_hicolor`).
"""
from __future__ import annotations

import io
import shutil
from pathlib import Path

import cairosvg
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "brand" / "app-icon.svg"
HICOLOR = ROOT / "packaging" / "icons" / "hicolor"
HICOLOR_SIZES = (16, 22, 24, 32, 48, 64, 96, 128, 256, 512)
TAURI = ROOT / "src-tauri" / "icons"


def render(size: int) -> Image.Image:
    png = cairosvg.svg2png(url=str(SRC), output_width=size, output_height=size)
    return Image.open(io.BytesIO(png)).convert("RGBA")


def save_png(img: Image.Image, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    img.save(path, format="PNG", optimize=True)


def main() -> None:
    renders = {n: render(n) for n in sorted({*HICOLOR_SIZES, 1024})}

    for n in HICOLOR_SIZES:
        save_png(renders[n], HICOLOR / f"{n}x{n}" / "apps" / "grokhub.png")
    for svg in (HICOLOR / "scalable" / "apps" / "grokhub.svg", ROOT / "packaging" / "grokhub.svg"):
        shutil.copyfile(SRC, svg)

    icons = ROOT / "packaging" / "icons"
    for n in (16, 22, 24, 32, 48, 64, 96, 128, 256, 512):
        save_png(renders[n], icons / f"grokhub-{n}.png")
    save_png(renders[256], icons / "icon.png")
    save_png(renders[512], icons / "icon-512.png")
    save_png(renders[32], icons / "tray.png")

    # Tauri shell bundles (tauri.conf.json `bundle.icon`, tray, window).
    save_png(renders[32], TAURI / "32x32.png")
    save_png(renders[128], TAURI / "128x128.png")
    save_png(renders[256], TAURI / "128x128@2x.png")
    save_png(renders[512], TAURI / "icon.png")
    renders[256].save(
        TAURI / "icon.ico",
        format="ICO",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (256, 256)],
    )
    renders[1024].save(TAURI / "icon.icns", format="ICNS")
    print("rendered icons from", SRC.relative_to(ROOT))


if __name__ == "__main__":
    main()
