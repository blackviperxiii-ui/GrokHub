# Brand

The one place the product's name, tagline, companion name and logo art live.
A rename is a one-line change to `brand.json`; new art is a file swap.

| File | What it is | Read by |
|------|------------|---------|
| `brand.json` | `productName`, `tagline`, `companionName` | `grokhub_core::brand` (cabin and shell) and `ui/src/brand.ts` |
| `logo-mark.svg` | The logo mark, drawn next to the name | `ui/src/brand.ts` |
| `app-icon.svg` | The app icon tile | `ui/src/brand.ts` |
| `companion.png` | The companion's still picture, docked top-right on the chat home: the Hanging out mood, shown while you're just talking | the cabin's home dock (`crates/grokhub-app/src/app/loadout_home.rs`) |
| `companion-hello.png` | The Hello mood, waving you in on an empty chat | the cabin's home dock |
| `companion-helping.png` | The Helping mood, checklist in hand while a tool runs | the cabin's home dock |

Swap art by replacing a file with one of the same name. Keep a square
`viewBox` on the SVGs so they scale cleanly. The companion pictures are
cutouts with a transparent background, 440 px tall; the dock fits each to
its frame.

What does not follow the brand: the config folder, the keychain service, the
binary names, the app ids and the shell's `tauri.conf.json` `productName`.
They stay `GrokHub` (`grokhub_core::brand::INTERNAL_ID`) so a rename never
moves anyone's settings, sign-ins or installed copy.

The starting art is the bracket-L mark and the slot-grid app icon from
`docs/design/loadout-baseline-v1/` (slides 3 to 5). The companion pictures are the deck's three poses (slide 17): `companion-hero`, `companion-pose2` and `companion-chill`, trimmed to their outline.
