; Windows installer for Core Board (Inno Setup 6).
; Built by .github/workflows/release.yml:  iscc /DAppVersion=x.y.z installer\coreboard.iss
; Kept as short as possible: one click to install, no admin rights needed, then it opens.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6B0E7C1E-3A57-4C3B-9C0F-6E2F1D4B8A21}
AppName=Core Board
AppVersion={#AppVersion}
AppVerName=Core Board {#AppVersion}
AppPublisher=TaikoGames
DefaultDirName={autopf}\Core Board
DefaultGroupName=Core Board
; Installs for the current user (no admin prompt) into their own Programs folder.
PrivilegesRequired=lowest
DisableWelcomePage=yes
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
OutputDir=..\dist
OutputBaseFilename=CoreBoard-Setup-{#AppVersion}
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\coreboard.exe
UninstallDisplayName=Core Board
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "Put a Core Board shortcut on the desktop"

[Files]
Source: "..\target\release\coreboard.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Core Board"; Filename: "{app}\coreboard.exe"
Name: "{autodesktop}\Core Board"; Filename: "{app}\coreboard.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\coreboard.exe"; Description: "Open Core Board"; Flags: nowait postinstall skipifsilent
; In-app updates run this installer silently: reopen Core Board when done.
Filename: "{app}\coreboard.exe"; Flags: nowait skipifnotsilent
