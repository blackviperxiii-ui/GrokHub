# Privacy

Loadout sends data only to the AI services and accounts the user configures, and only when the user asks. It collects no telemetry and transfers no information to the maintainer or other networked systems.

Your chats, memory, and keys stay on your computer.

## On-device model

When the on-device model is on in Settings → AI models:

- Online, the cloud takes every step while any cloud model answers. This PC's model is the last resort, never for hard or long work.
- Offline, this PC's model takes what it can. Work that needs the cloud waits for the network.
- Sensitive data you haven't allowed to the cloud stays on this PC.
- The model loads only when it's needed and unloads when it sits idle.
- Explicit 18+ chats go only to xAI or this PC's model.

## Updates

- Every few hours Loadout asks GitHub for this project's release list to see if a newer version is out. The request carries no account, chat or key.
- An update installs only when you press Install, and only if its signature matches the project's key.
- Offline-only mode skips the check entirely.
