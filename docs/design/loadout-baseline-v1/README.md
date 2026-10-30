# Loadout visual baseline v1 (locked)

This folder is the locked visual reference for the Loadout rebrand of the desktop app. Loadout is a working name and may change, so the build keeps the product name, the companion name ("Nyx") and the logo and icon art swappable from one central place.

Every Loadout design and build task must match these slides. Don't edit the files here; propose changes as a new folder.

Direction: gaming-flavored chrome (black #0B0B0D, red #D7263D outlines, gray #2A2B2F hover and #3A3B40 selected states, amber #FFB000 accents) around a chat-forward, companion-led agent. Gaming is a strong flavor alongside talking, chatting and hanging out with the agent, never a replacement for it.

## Files

- `slide-01.png` to `slide-20.png`: the 20 slides.
- `deck.html`: the whole deck, offline. Arrow keys, Space or click change slides, F toggles fullscreen, L opens the live Pulse page.
- `pulse-live.html`: the reference implementation of the responsive Pulse layout in real CSS. Resize the window to watch it reflow.
- `assets/`: SVG marks and icons, companion art and fonts used by the deck.

## Slides

| # | Slide |
|---|-------|
| 01 | Title: Loadout lockup |
| 02 | Hero: Hangout home. Chat-forward, with Objectives, PC loadout and From Pulse panels alongside, and the companion docked top-right ("Here · tap to talk", Listening waveform) as a still picture |
| 03 | Logo: primary bracket-L mark and wordmark (the starting point), plus alternates |
| 04 | Swappable name system |
| 05 | App icon directions; the slot grid icon is the starting point |
| 06 | 3D icon render study (gold, red rims, black, amber light) |
| 07 | Palette and UI tokens |
| 08 | Themes: Ops Dark (primary), OLED Black, Soft Charcoal |
| 09–12 | Layouts A–D: mission control HUD, chat-first with objectives, game library hub, privacy and local data |
| 13 | Pulse in the Loadout style: same name and functions (PC care and gaming content), fluid grid |
| 14 | Pulse at 2560, 1440, 1024 and 600 px, plus the breakpoint rules |
| 15 | Pulse card close-up and the list of kept functions |
| 16 | Companion hero on a semi-holographic pedestal. The cabin draws its pedestal in code (`crates/grokhub-app/src/pedestal.rs`); `scripts/deck-shots.ps1 -pedestal` previews it in the corner box in each theme |
| 17 | Companion moods: Hello, Helping, Hanging out |
| 18 | Companion as a live wallpaper (parked idea, not part of the foundation) |
| 19 | The 21 existing personalities, all kept |
| 20 | Recommended combo: gaming chrome plus a chat-forward home |

The deck predates the companion name "Nyx" and the new default personality; the build adds both without a deck change.
