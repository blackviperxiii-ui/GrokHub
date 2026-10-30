# Companions

One folder per companion, each with a `companion.toml`
(`grokhub_core::companion`). Nyx is the only one today; the format leaves
room for more.

```toml
id = "nyx"
name = "Nyx"
pedestal_tint = "#5ad1ff"      # optional; the theme accent when blank
default_outfit = "hanging-out"

[[outfit]]
id = "working"
file = "brand/nyx-working.jpg"  # shipped art, relative to the repo root
moods = ["working"]
adult = false                   # never true in the repo (CI guard 80)

# Optional patches for blinking and talking. Anchors are the patch's
# top-left corner in the outfit picture's own pixels.
[outfit.eyes]
file = "brand/nyx-working-eyes-closed.png"
x = 180
y = 96

[outfit.mouth]
closed = "brand/nyx-working-mouth-closed.png"
half = "brand/nyx-working-mouth-half.png"
open = "brand/nyx-working-mouth-open.png"
x = 204
y = 150
```

Modes: `working`, `gaming`, `doctor`, `reading`, `hanging-out`, `viper`,
`thinking`, `writing`, `raunchy`. Nyx ships one outfit per mode; a mode with
no outfit of its own shows the default outfit. Only outfits with eye and mouth patches can blink and
talk; the rest still breathe and bob.

## Adults-only outfits

Adult art never goes in this public repo. It loads only from the user's
own PC, from `{config}/companions/<id>/adult/adult.toml`:

```toml
[[outfit]]
id = "raunchy"
file = "raunchy.png"            # relative to the adult folder
moods = ["raunchy"]
```

Every outfit read from that folder is adult, whatever the file says, and
the app shows it only with adults-only personalities switched on.
`scripts/ci-guards.d/80-companion-adult-art.sh` fails CI if a tracked
file lands under a `companions/**/adult/` path or a shipped
`companion.toml` marks an outfit `adult = true`.

## Art prep

The desktop companion needs a background-removed cutout per outfit, with
a transparent background, about 480 px tall. The shipped `brand/nyx-*.jpg`
pictures are full scenes, so until the cutouts land the desktop shows the
same framed picture the corner box does.
