# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

## Scope

SignPath signs GrokHub-owned Windows release binaries built by GitHub Actions from [blackviperxiii-ui/GrokHub](https://github.com/blackviperxiii-ui/GrokHub):

- `GrokHub-Setup-*.exe` (Inno Setup installer)
- `grokhub.exe` and `grokhub-hub.exe` inside `grokhub-windows-v*.zip`

Signing runs only on `v*` tags when SignPath GitHub secrets/variables are configured. Branch pushes to `beta` publish unsigned Windows pre-release assets (checksums / minisign on `SHA256SUMS` still apply when configured).

Artifact layout and SignPath XML: [`.signpath/windows-release.xml`](../.signpath/windows-release.xml). Owner application checklist: see the Project store doc `docs/signpath-oss-signing.md` (not in this repo).

## Team roles

- **Committers and reviewers:** repository collaborators with write access to `blackviperxiii-ui/GrokHub`.
- **Approvers:** repository owners who hold SignPath approval rights for the GrokHub signing policy (each release signing request is approved manually).

## Privacy

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it. Chat, memory, and keys stay on the user’s computer. Optional features (account sign-in, updates, Pulse, connectors) contact remote services only when the user enables or uses them.

## SmartScreen

The certificate is issued to SignPath Foundation (OV-class). A valid signature does not instantly clear Microsoft SmartScreen; reputation builds with downloads over time.
