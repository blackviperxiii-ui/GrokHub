# Deck-match proof shots

Side-by-side captures of the running Windows cabin (`grokhub.exe`) next to the locked deck slides in `docs/design/loadout-baseline-v1/`.

Each change keeps its own page here, so two changes never edit the same file:

- Add `docs/design/deck-match/<slug>.md` with a `| File | Size | Slide |` table for your shots, and put the images beside it.
- Don't add rows to this README. It only explains the layout.
- List every page with `ls docs/design/deck-match/*.md`.

Shots come from `scripts/deck-shots.ps1` on the Windows CI job (`deck-shots` artifact, and base64 JPEGs between `DECK-SHOT-BEGIN` / `DECK-SHOT-END` markers in the job log). The shot list lives in `scripts/deck-shots.list.ps1`.
