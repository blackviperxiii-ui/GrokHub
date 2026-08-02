# GrokHub on Windows

## Recommended: download the `.exe`

1. Go to [Releases](https://github.com/blackviperxiii-ui/Grok-Hub/releases)
2. Download **GrokHub-Setup-*.exe**
3. Double-click to install
4. Open **GrokHub** from Start Menu or Desktop

Portable: **GrokHub-Portable-*.exe** (runs without installing).

No Node.js or Git required.

SmartScreen warning on unsigned builds → **More info** → **Run anyway**.

## CI packaging

GitHub Actions: `.github/workflows/release-windows.yml`

- Trigger: push tag `v*`, or **Actions → Release Windows → Run workflow**
- Produces NSIS setup + portable exe and attaches them to a GitHub Release

## From source (developers)

```powershell
npm install
npm run desktop:build
npm install -D electron@35.1.2 electron-builder
npm run dist:win
```

Output: `dist/GrokHub-Setup-*.exe`, `dist/GrokHub-Portable-*.exe`

## Legacy PowerShell install

`install.ps1` / `download-and-install.ps1` still work for source installs, but the **Release .exe** is preferred for end users.
