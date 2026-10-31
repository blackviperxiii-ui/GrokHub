# Brand

The one place the product's name, tagline, companion name and logo art live.
A rename is a one-line change to `brand.json`; new art is a file swap.

| File | What it is | Read by |
|------|------------|---------|
| `brand.json` | `productName`, `tagline`, `companionName` | `grokhub_core::brand` (cabin and shell) and `ui/src/brand.ts` |
| `logo-mark.svg` | The logo mark, drawn next to the name | `ui/src/brand.ts` |
| `app-icon.svg` | The app icon tile | `ui/src/brand.ts` |
| `companion.png` | The companion cutout the chat header's round face is cropped from | the cabin's chat header (`crates/grokhub-app/src/app/loadout_home.rs`) |
| `nyx/actions/<mode>/<n>.jpg`, `nyx/defaults/<preset>/<n>.jpg` | Nyx's other poses: more per mode (plus Thinking and Writing), and each built-in personality's outfit, shown in every mode and on its picker tile. The folders are mapped in `docs/design/loadout-baseline-v1/nyx-modes/README.md` | the cabin's home dock (`crates/grokhub-app/src/app/loadout_home/nyx_wardrobe.rs`) and Settings → Personality |
| `nyx-working.jpg`, `nyx-gaming.jpg`, `nyx-doctor.jpg`, `nyx-reading.jpg`, `nyx-hanging-out.jpg` | Nyx in the home dock's corner box, one per mode: a coding tool, a game you just left, a PC-care tool, a search or read, and just talking | the cabin's home dock |
| `nyx-viper.jpg` | Nyx in the snake onesie, shown in every mode while the Viper personality is on | the cabin's home dock |
| `nyx-raunchy.jpg` | Nyx in the raunchy outfit, shown in every mode while another explicit personality is on and adults-only personalities are switched on | the cabin's home dock |

Swap art by replacing a file with one of the same name. Keep a square
`viewBox` on the SVGs so they scale cleanly. `companion.png` is a cutout
with a transparent background, 440 px tall. The `nyx-*.jpg` and `nyx/` pictures are
448 x 350 crops of the approved scenes in
`docs/design/loadout-baseline-v1/nyx-modes/`, framed on her face and upper
body; the dock fills its frame with the middle of each. Which picture each
mode wears is data in `crates/grokhub-core/assets/companions/nyx/companion.toml`
(see the README beside it); the dock only embeds the files that names.

What does not follow the brand: the config folder, the keychain service, the
binary names, the app ids and the shell's `tauri.conf.json` `productName`.
They stay `GrokHub` (`grokhub_core::brand::INTERNAL_ID`) so a rename never
moves anyone's settings, sign-ins or installed copy.

The starting art is the bracket-L mark and the slot-grid app icon from
`docs/design/loadout-baseline-v1/` (slides 3 to 5). `companion.png` is the deck's `companion-chill` pose (slide 17), trimmed to its outline.
