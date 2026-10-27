# Brand

The one place the product's name, tagline, companion name and logo art live.
A rename is a one-line change to `brand.json`; new art is a file swap.

| File | What it is | Read by |
|------|------------|---------|
| `brand.json` | `productName`, `tagline`, `companionName` | `grokhub_core::brand` (cabin and shell) and `ui/src/brand.ts` |
| `logo-mark.svg` | The logo mark, drawn next to the name | `ui/src/brand.ts` |
| `app-icon.svg` | The app icon tile | `ui/src/brand.ts` |
| `companion.png` | The companion's still picture, docked top-right on the chat home | the cabin's home dock (`crates/grokhub-app/src/app/loadout_home.rs`) |

Swap art by replacing a file with one of the same name. Keep a square
`viewBox` on the SVGs so they scale cleanly. `companion.png` is a cutout
with a transparent background, about 350 by 440 px; the dock fits it to its
height.

What does not follow the brand: the config folder, the keychain service, the
binary names, the app ids and the shell's `tauri.conf.json` `productName`.
They stay `GrokHub` (`grokhub_core::brand::INTERNAL_ID`) so a rename never
moves anyone's settings, sign-ins or installed copy.

The starting art is the bracket-L mark and the slot-grid app icon from
`docs/design/loadout-baseline-v1/` (slides 3 to 5). The companion picture is the deck's chill pose (slide 2).
