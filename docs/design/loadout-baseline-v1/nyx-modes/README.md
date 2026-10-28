# Nyx modes — companion corner-box photo sets

Grok Bot supplies the final art. Drop stills here; the cabin embeds copies
under `crates/grokhub-app/assets/companion-photos/` (same layout). A later
pass turns each still into a short clip (Grok Imagine); the dock looks up a
still today and can swap in a clip path with the same name later.

## Layout

```
defaults/<personality-id>/1.png … 5.png   # idle / talking rotation
actions/<action>/1.png … 5.png            # while Nyx is on a task
```

Actions: `coding`, `fixing`, `reading`, `writing`, `working`, `thinking`,
`gaming`.

## Placeholders shipping now

- Nyx defaults 1–3 start from the Loadout deck companion cuts
  (`assets/companion-hero`, `companion-chill`, `companion-pose2`).
- Defaults 4–5 and every action slot use brand companion poses until Grok Bot
  art lands. Other personalities fall back to Nyx's default set at runtime.

Do not block Default→Nyx migration on missing final art.
