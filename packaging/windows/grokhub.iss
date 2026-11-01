#define MyAppName "Loadout"
#ifndef MyAppVersion
  #define MyAppVersion "3.5.0"
#endif
#define MyAppPublisher "Loadout"
#define MyAppExeName "grokhub.exe"

[Setup]
AppId={{9E2C7C3A-4B11-4F6F-9D3A-6A1C8F0B2E44}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\GrokHub
DefaultGroupName=Loadout
; The start menu group used to be GrokHub; a fresh group, and [InstallDelete]
; below clears the old shortcuts. AppId, the install folder and the Setup file
; name stay as they were so updates and the in-app updater keep working.
UsePreviousGroup=no
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\..\dist-release
OutputBaseFilename=GrokHub-Setup-{#MyAppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
InfoBeforeFile=privacy.txt
SetupIconFile=grokhub.ico
Uninstallable=yes
UninstallDisplayIcon={app}\{#MyAppExeName}
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "stage\grokhub.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "stage\grokhub-hub.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "stage\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[InstallDelete]
Type: files; Name: "{autoprograms}\GrokHub\GrokHub.lnk"
Type: dirifempty; Name: "{autoprograms}\GrokHub"
Type: files; Name: "{autodesktop}\GrokHub.lnk"

[Icons]
Name: "{group}\Loadout"; Filename: "{app}\{#MyAppExeName}"; AppUserModelID: "GrokHub.Cabin"
Name: "{autodesktop}\Loadout"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon; AppUserModelID: "GrokHub.Cabin"

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch Loadout"; Flags: nowait postinstall skipifsilent
