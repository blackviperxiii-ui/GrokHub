# Built-in model profiles

The router starts every cloud model from a profile, so it doesn't have to learn each one from scratch. Live measurement then adjusts the priors as calls come in.

- **Cloud models:** `crates/grokhub-core/data/model_profiles.toml`, compiled into the app.
- **Local models:** `crates/grokhub-core/data/local_catalog.toml` (skills and badges), updated through the signed catalog feed.

## What a row holds

Each `[[model]]` row is one model, keyed by its wire id (`grok-4.7`, `claude-sonnet-5.5`, `deepseek/deepseek-v4.1-flash-1017`). Every field except `key` is optional, and a missing field means unknown, never zero.

| Field | Meaning |
| --- | --- |
| `provider` | `xai`, `anthropic` or `openrouter` |
| `aliases` | Other ids for the same model (`claude-sonnet-5-5`, `grok-code-fast-1`) |
| `mirror` | The model's OpenRouter id, for the updater's cross-check |
| `tier` | `frontier`, `flagship`, `workhorse`, `fast`, `paid` or `free` |
| `intelligence` | Artificial Analysis Intelligence Index (0-100) |
| `roles` | Purpose tags the scorer reads (`long`, `free_code`, `never_default`, …) |
| `caps` | Capability priors, 0-100: chat, code, tools, reasoning, long_context, vision, audio |
| `lane` | Grok lane picker seeds (set on the derived lane caps; `speed_floor` only raises speed) |
| `prompt`, `cached`, `cache_write`, `completion` | USD cents per 100M tokens ($1 per 1M = 10_000) |
| `prompt_long`, `completion_long`, `long_threshold` | The long-prompt price step |
| `context`, `max_output` | Window and output cap, in tokens |
| `input`, `output` | Modalities |
| `tools`, `structured`, `reasoning` | Tool calling, structured output, reasoning |
| `efforts`, `default_effort` | Reference only; efforts are applied from a provider's own listing |
| `trains`, `retains`, `moderated` | Data policy |
| `expires`, `hosts`, `probation`, `speed_tps` | Lifecycle and speed priors |
| `source`, `verified` | Where the facts came from and the day they were checked |
| `estimate` | Fields that are judgment, not a provider's stated fact |
| `pin` | Fields that beat a provider's listing |
| `gone_since` | The day the provider stopped listing the model |
| `note` | Free text. The file has no comments, because the updater rewrites it |

## Which value wins

1. A user override (see below).
2. A pinned profile field.
3. The provider's own listing (xAI API, OpenRouter, Anthropic).
4. The profile.

Direct xAI prices and windows come only from the xAI API listing, or from the profile when only Grok Build lists the model. A row never mixes the two, and an xAI API row with nothing in it stays a ghost.

## Updating

- **Automatic:** `.github/workflows/model-profiles.yml` runs every Monday. It pulls OpenRouter's public `/api/v1/models` list, updates the facts on OpenRouter rows (prices, window, max output, modalities, tool support, expiry), adds new free models on probation, stamps `gone_since` on models that left the list, and opens a PR to beta. The job refuses to write when the list is missing most rows, which means a bad fetch.
- **By hand:**
  ```
  python3 .github/scripts/model_profiles.py check    # print drift
  python3 .github/scripts/model_profiles.py write    # apply it
  python3 .github/scripts/model_profiles.py format   # canonical layout after a hand edit
  ```
  Direct Claude and xAI rows have no public price list. Check them against the providers' pricing pages, set `verified`, and run `format`. `check` reports when a direct row differs from its OpenRouter mirror.
- **A new model:** add a row with the provider's facts, set `source` and `verified`, put judgment values in `caps` and list `caps` under `estimate`. The tests in `model_registry::builtin` and `test_model_profiles.py` check the file.

## User overrides

`{config}/models/profile_overrides.toml` uses the same schema (`schema = 1`, then `[[model]]` rows). An override row's fields replace the built-in row's and are pinned, so they beat listings too. A key the profiles don't have adds a row. A file that doesn't parse is ignored as a whole. The router reads it when the model registry loads.

## Local models

Every on-device chat model in `crates/grokhub-core/data/local_catalog.toml` carries a profile too, read into `LocalModel::profile`:

- `params_b`, `active_b`: billions of parameters, all and active per token (they differ for a mixture of experts).
- `ctx_max`: the context window it was trained for, known before the download.
- `tok_s_tier`: estimated tok/s on each hardware tier's reference GPU (T1 6 GB, T2 8 GB, T3 16 GB, T4 24 GB), only on tiers whose VRAM holds it. The hot-swap lineup (`model_lineup::est_tok_s`) starts from the figure for this PC's tier; the setup wizard's default still uses `tok_s`, and a speed test replaces either.
- `gaming_safe`: at most 4.5 GiB at the default context and quality 38 or more. A game swaps to the lightest gaming-safe model below the one in use, and to the lightest one when none is.
- Tool calling, memory need, quality and tier were already on the row (`badges`, `run_mb`, `min_budget`, `skills`, `tier`); `LocalModel::tools()` reads the badge.

Everything above is an estimate until measured: `estimate` lists the fields, and a field moves to `measured` once someone measures it. To add or change a model, set its `params_b`, `active_b` and `ctx_max`, then:

```
python3 .github/scripts/local_profiles.py write   # derive tok_s_tier, gaming_safe, estimate
python3 .github/scripts/local_profiles.py check   # what CI runs
```

Raise the catalog's `version` with any change, so a newer signed catalog reaches older apps.
