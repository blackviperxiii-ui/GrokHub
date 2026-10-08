# Spike-2b: click targets and tool search

Status: built and unit-tested. Not run against a live desktop, a live Cua Driver, or Grok Build yet.

## What a click will do

`click_target_class(effect, label, role)` in `crates/grokhub-agent/src/harness/hard.rs`. A declared hard `effect` (Cua tool metadata) beats the label; a declared soft effect never makes a hard label soft (stricter only, D1). Labels match as whole words, any case, like `credential_field`. Unknown or empty labels are soft. The rule id (`send:Send`) is all a span keeps.

| Class | Label words | Stays soft |
|---|---|---|
| send | Send, Resend, Post, Publish, Reply, Tweet, Share, Upload; Submit when the window or label says message, compose, mail, chat, reply, comment, post, draft | Sender, Sent, Shared, Posts, Undo send |
| money | Pay, Buy, Purchase, Place order, Checkout, Confirm payment, Subscribe, Donate, Transfer; Submit when the window or label says checkout, payment, cart, billing, order | Payload, Payment methods, Unsubscribe |
| delete | Delete, Remove, Empty trash, Empty bin, Empty recycle bin, Erase, Discard | Removed items, Deleted items |
| irreversible_os | Reset, Factory reset, Wipe, Format (alone or on a disk, drive, partition, volume, USB, SD card, device, storage) | Reset zoom / view / filter / search / sort / layout / columns / selection / font / scale, Format cells, Format painter |

Any label that starts with Undo, Don't or Do not is soft, and so is a label whose role only shows text (label, static text, heading, paragraph, tooltip, status bar, title bar).

## Where the label comes from

| Path | Source |
|---|---|
| A, Linux | `DesktopServer::click_target` maps the click into screen space and calls `DesktopBackend::target_at`: the existing AT-SPI walk (`ATSPI_PY`, `parse_atspi_line`) through `run_limited`, capped at 300 ms, then `control_at` (smallest interactive control under the point). A timeout or no match is unknown: soft, span `target:"unknown"`. |
| A, Windows | The trait default: unknown, soft, logged. No UIA reader (D3). |
| Cua | The `element_index` the click names, looked up in the `get_window_state` reply the proxy already takes before a click (structured `elements`, or `[N] role "label"` lines). Not yet checked against a live 0.34.0 driver. |
| B | The ask card: the first quoted text in the action, else the words after "click". |
| D | The frame's own `label` / `element` args through `desk_classify`. |
| E | A native `click` that names its `label` / `element`. |

The hard card is the existing one: title is the class label, the action line says "Grok wants to click Send in <window>". No new card, chip, page or `Nav` variant.

## Batches

Grok Build sends one desktop or Cua call at a time, so the batch rule lives in the native engine (path E): when a hard step in one model turn is denied (or refused while unattended), the rest of that turn's calls return "Not executed: earlier action failed" and never run.

## Tool search

`DEFER_AFTER` (40) now counts the native desktop tools (7 with desktop control on) and every MCP tool (Cua included) together. Past it, MCP tools go behind `search_tool` / `use_tool`; `use_tool` is classified as the tool it names, so a hard tool still parks under Always.

## Router-ready

This spike adds and touches no model calls (the classifier is pure, the gate reads files and the AX tree), so no `ModelCall` wrapper was added; a wrapper with no caller would be dead code.
