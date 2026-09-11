#define MyAppName "GrokHub"
#ifndef MyAppVersion
  #define MyAppVersion "2.9.3"
#endif
#define MyAppPublisher "GrokHub"
#define MyAppExeName "grokhub.exe"

[Setup]
AppId={{9E2C7C3A-4B11-4F6F-9D3A-6A1C8F0B2E44}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\GrokHub
DefaultGroupName=GrokHub
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\..\dist-release
OutputBaseFilename=GrokHub-Setup-{#MyAppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile=grokhub.ico
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
Source: "stage\grok.exe"; DestDir: "{%USERPROFILE}\.grok\bin"; Flags: ignoreversion skipifsourcedoesntexist
Source: "stage\agent.exe"; DestDir: "{%USERPROFILE}\.grok\bin"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{group}\GrokHub"; Filename: "{app}\{#MyAppExeName}"; AppUserModelID: "GrokHub.Cabin"
Name: "{autodesktop}\GrokHub"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon; AppUserModelID: "GrokHub.Cabin"

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch GrokHub"; Flags: nowait postinstall skipifsilent

[Registry]
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; \
  ValueData: "{olddata};{%USERPROFILE}\.grok\bin"; Flags: preservestringtype; \
  Check: NeedsGrokPath

[Code]
function NeedsGrokPath: Boolean;
var
  P: String;
begin
  P := GetEnv('PATH');
  Result := Pos(ExpandConstant('{%USERPROFILE}\.grok\bin'), P) = 0;
end;
