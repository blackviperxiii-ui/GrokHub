# Spacing

One spacing scale for both Loadout apps: the egui cabin and the Tauri shell.

## The scale

| Step (px) | Use |
|---|---|
| 2 | Nudges inside a control (icon to text baseline) |
| 4 | Tight pairs: a label and its value, chip contents |
| 8 | Default gap between items in a row or a list |
| 12 | Padding inside small cards and pills; gap between related groups |
| 16 | Card padding; gap between cards; page margin on a narrow window |
| 24 | Page margin; gap between page sections; sheet padding |
| 32, 40, 48 | Space above a new page section, empty states |
| 64, 80, 96 | Hero and onboarding layouts |

0 and 1 (hairlines) are allowed. Nothing else is: an off-scale gap snaps to the
nearest step, with ties rounding up (6 becomes 8, 10 becomes 12, 14 becomes 16).
The scale follows IBM Carbon's (2, 4, 8, 12, 16, 24, 32, 40, 48, 64, 80, 96).

- Cabin: `theme::SPACING_*`, `theme::PAGE_MARGIN`, `theme::page_margin()`.
- Shell: `--space-*` and `--page-pad` in `ui/src/styles.css`.

## Rules

- **Proximity:** space inside a group is smaller than space between groups.
  A label sits 4 from its value, items sit 8 apart, groups sit 16 or 24 apart.
  If two groups are as close as their own items, they read as one group.
- **Targets:** nothing you click is shorter than 24px (WCAG 2.2, 2.5.8). Kit
  controls keep `MIN_HIT` (28); the main rail and toolbar keep `HIT` (40).
  Density never shrinks a target, only the space around it (Material density).
- **Auto density, no setting:** pages keep a 24 margin on a wide window and 16
  below 900 logical px, so a laptop window keeps its content width.
- **Radii** match too: 8, 12 and 16 in both apps.

## Keeping it

- `scripts/spacing_snap.py rust <files>` and `scripts/spacing_snap.py css <files>`
  snap literals onto the scale (`--check` reports only).
- `every_cabin_gap_is_on_the_spacing_scale` (cabin) and `styles.spacing.test.ts`
  (shell) fail on any new off-scale gap.
