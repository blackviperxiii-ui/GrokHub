# Loadout palette (locked)

The Tauri shell’s dark themes use this token table. Values live in
`ui/src/styles.css`. Soft Charcoal keeps the warmer red / dim from the deck;
Ops Dark and OLED Black share the primary red.

| Token | Value | Role |
| --- | --- | --- |
| `--void` | `#0A0A0B` | OLED base; text on amber fills |
| `--bg` | `#0B0B0D` | App background |
| `--surface` / `--panel` | `#121215` | Panels, rails |
| `--raised` | `#18191C` | Cards, user bubble |
| `--red` / `--primary` | `#D7263D` | Primary outlines, pressed/selected, focus ring, links |
| `--red-hover` | `#E8384F` | Brighter stroke; red *text* (4.78:1 on `--bg`) |
| `--red-dim` | `#5E1A25` | Secondary outlines (small tiles, chips) |
| `--red-wash` | `rgb(215 38 61 / .10)` | Pressed / selected fill |
| `--red-glow` | `0 0 0 1px rgb(215 38 61/.18), 0 0 18px rgb(215 38 61/.22)` | Focus / hover glow |
| `--amber` | `#FF9F1A` | One CTA fill per view, objectives, XP, toggles on, warnings (≤10% of screen) |
| `--amber-wash` | `rgb(255 159 26 / .12)` | Objective tile tint |
| `--line` | `#3A161D` | Neutral hairlines (red-tinted; replaces gray `#232428`) |
| `--hover` | `#1F1316` | Red-tinted hover |
| `--selected` | `#2A1519` | Red-tinted selected |
| `--text` / `--steel` / `--muted` | `#F2F2F4` / `#C9CBD1` / `#8A8C94` | Keep; muted stays neutral for legibility |
| `--danger-text` | `#FF4D5E` | Destructive label text (must differ from primary by more than color) |

`--accent` points at `--red` with light `--accent-text` on the fill.
`--border` points at `--line`. Soft Charcoal uses red `#C8304A` and dim
`#5A2530`.

Contrast notes: `#D7263D` on `#0B0B0D` is 3.96:1 (OK for strokes at 3:1, not
for small text at 4.5:1). Use `--red-hover` for red text. Amber `#FF9F1A` is
9.58:1 on black.

Button variants (outline default + single amber CTA) are a later card
(LO-UI-3). Visual baseline slides: `docs/design/loadout-baseline-v1/`.
