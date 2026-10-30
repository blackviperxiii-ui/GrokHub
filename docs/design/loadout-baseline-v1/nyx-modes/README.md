# Nyx modes and outfits

Approved 1024 x 1024 originals of Nyx for the cabin's corner box (the home
dock) and the Settings → Personality picker. The app embeds 448 x 350 crops
of them from `brand/nyx-*.jpg` and `brand/nyx/`, at the same paths with
`.jpg` in place of `.png`. The code that picks one is
`crates/grokhub-app/src/app/loadout_home/nyx_wardrobe.rs`.

## Nyx's modes (`actions/`)

Nyx follows what the chat is doing. Each mode has a set of poses and shows
one; the next pose comes on the next time that mode does.

| Folder | Mode | Shown when |
|--------|------|------------|
| `working-coding.png`, `actions/coding/2-5`, `actions/working/1-5` | Working | a tool is running that is not PC care, a lookup or writing |
| `fixing-doctor.png`, `actions/fixing/2-5` | Doctor | a PC-care tool (drivers, temps, repair) |
| `reading-research.png`, `actions/reading/2-5` | Reading | a search, fetch or read |
| `actions/writing/1-5` | Writing | a prose tool (draft, compose, essay, letter, notes, journal) |
| `actions/thinking/1-5` | Thinking | a reply is running and no tool has started yet |
| `gaming.png`, `actions/gaming/2-5` | Gaming | a game the user just left |
| `hanging-out.png`, `defaults/nyx/4-5` | Hanging out | anything else |

`defaults/nyx/1-3` are the deck's companion poses in `../assets/`
(`companion-hero`, `companion-chill`, `companion-pose2`).

## Personality outfits (`defaults/`)

Every other built-in personality is Nyx in that voice's outfit: the folder
name is the preset id in `crates/grokhub-core/assets/personalities.toml`. The
corner box shows that set in every mode, and the picker tile shows pose 1.

| Folder | Preset |
|--------|--------|
| `defaults/bard/1-5` | Shakespearean Bard |
| `defaults/butler/1-5` | Butler |
| `defaults/chief-engineer/1-5` | Chief Engineer |
| `defaults/concise-pro/1-5` | Concise Pro |
| `defaults/drill-sergeant/1-5` | Drill Sergeant (lite) |
| `defaults/friendly-neighbor/1-5` | Friendly Neighbor |
| `defaults/genie/1-5` | Genie |
| `defaults/gentle-listener/1-5` | Gentle Listener |
| `defaults/hype-coach/1-5` | Hype Coach |
| `defaults/lawyer-precise/1-5` | Lawyer-precise |
| `defaults/mission-control/1-5` | Space Mission Control |
| `defaults/noir-detective/1-5` | Noir Detective |
| `defaults/pirate/1-5` | Pirate |
| `defaults/professor/1-5` | Professor |
| `defaults/retro-hacker/1-5` | Retro 80s Hacker |
| `defaults/sarcastic-sidekick/1-5` | Sarcastic Sidekick |
| `defaults/southern-mechanic/1-5` | Southern Mechanic |
| `defaults/surfer/1-5` | Surfer |
| `defaults/zen-mentor/1-5` | Zen Mentor |
| `viper-snake-onesie.png`, `defaults/viper/2-5` | Viper (explicit) |
| `defaults/whore/1-5` | Whore (explicit) |

The explicit presets' pictures show only while Settings → Adults-only
personalities is on and the one-time 18+ check was answered yes.

## Reference only

`raunchy-v1.png` is an earlier outfit kept for reference. Nothing shows it.
