# Type

One type scale for both Loadout apps: the egui cabin (`theme::TYPE_*`, `LH_*`)
and the Tauri shell (`--font-*`, `--lh-*`). Inter throughout.

## The scale

| Role | Size / line height (px) | Use |
|---|---|---|
| badge | 10 | Unread-counter digits only (`TYPE_BADGE`) |
| caption | 12 / 16 | Tips, meta lines, timestamps, small labels |
| body | 14 / 20 | UI text: labels, list rows, inputs, buttons |
| prose | 16 / 24 | Reading text: chat replies, the composer, markdown, notes |
| title | 16 / 24, semibold | Card and row titles |
| section | 20 / 28, semibold | Section headers; a markdown `#` heading |
| headline | 24 / 32, semibold | Page titles, the Home greeting |
| display | 32 / 40, semibold | Big numbers and hero lines |

Every line height sits on the 4px spacing grid ([spacing.md](spacing.md)), so
text lines up with gaps. Prose gets 1.5x leading (WCAG 1.4.8); UI rows stay
tighter so lists don't sprawl.

Weights: 400 regular, 500 medium (buttons, tabs), 600 semibold (titles). No 700.

An off-scale size snaps to the nearest step, ties rounding up (13 becomes 14,
15 becomes 16, 18 becomes 20, 22 becomes 24, 28 becomes 32):
`scripts/type_snap.py rust|css <files>`.

## Window size

Below 900px wide, page titles step down one level (headline 24 to section 20)
and display numbers too (32 to 24), so titles keep to one line on a small
laptop window. Body and prose never shrink. No setting: it follows the window,
the same breakpoint as the page margin.

## Bigger text

Settings → Appearance → Text size zooms the whole app, and auto-tune raises it
from the OS text size (Windows text size, GNOME text scaling, KDE font DPI).
The shell sizes text in rem, so zoom and the OS setting scale it (WCAG 1.4.4).

## Guards

- Cabin: `theme_type_tests.rs` fails on any literal `.size(N)`, `FontId` size
  or font-size const that is not a step.
- Shell: `styles.type.test.ts` fails on any `font-size` that is not a
  `--font-*` token, any `line-height` that is not a `--lh-*` token (1 is allowed
  for icon glyphs), and any weight other than 400, 500 or 600.

## Sources

- Fluent 2 type ramp (12/16, 14/20, 16/22, 20/28, 24/32, 32/40):
  fluent2.microsoft.design/typography
- Material 3 type scale (body 14/20 and 16/24, title 16/24, headline 24/32).
- WCAG 2.2: 1.4.4 Resize Text, 1.4.8 Visual Presentation, 1.4.12 Text Spacing.
- Inter dynamic metrics (rsms): tighter tracking as text gets bigger.
