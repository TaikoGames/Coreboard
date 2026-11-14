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
; A running Core Board is renamed out of the way instead (see [Code]), so updates never wait on it.
CloseApplications=no

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

[UninstallDelete]
; Copies moved aside by updates while Core Board was open.
Type: files; Name: "{app}\coreboard.old*.exe"

[Code]
// Windows won't overwrite a program that's running, but it will rename it. Moving the open
// Core Board aside lets an in-app update install right away; the new version deletes the old
// copy the next time it starts.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Exe, Old: String;
  I: Integer;
begin
  Result := '';
  Exe := ExpandConstant('{app}\coreboard.exe');
  if FileExists(Exe) and not DeleteFile(Exe) then
  begin
    I := 0;
    repeat
      Old := ExpandConstant('{app}\coreboard.old') + IntToStr(I) + '.exe';
      DeleteFile(Old);
      I := I + 1;
    until RenameFile(Exe, Old) or (I > 20);
    if FileExists(Exe) then
      Result := 'Core Board is open and could not be updated. Close it and try again.';
  end;
end;
