; The Windows installer, built by `cargo xtask package` with Inno Setup 6 (https://jrsoftware.org).
; It installs for the person running it unless they choose everyone, asks whether to put
; OpenMailClient on the Start menu and the desktop, and is what Check for Updates runs to update:
; `/SILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS /RELAUNCH=1`, the earlier choices kept.
; Mail, settings and accounts are never touched: they are not in the program's folder, and
; uninstalling leaves them where they are.

#ifndef AppVersion
  #error Build with cargo xtask package, which passes /DAppVersion, /DNumericVersion and /DSourceDir
#endif

[Setup]
; Never change the AppId: it is how a later installer knows it updates this one.
AppId={{40C3C759-D101-4DE6-969E-4C644BCC8D68}
AppName=OpenMailClient
AppVersion={#AppVersion}
AppVerName=OpenMailClient {#AppVersion}
AppPublisher=OpenMailClient contributors
AppPublisherURL=https://github.com/AdriaanBoshoff/OpenMailClient
AppSupportURL=https://github.com/AdriaanBoshoff/OpenMailClient/issues
AppUpdatesURL=https://github.com/AdriaanBoshoff/OpenMailClient/releases
VersionInfoVersion={#NumericVersion}
VersionInfoProductVersion={#NumericVersion}
VersionInfoProductTextVersion={#AppVersion}
VersionInfoDescription=OpenMailClient Setup
DefaultDirName={autopf}\OpenMailClient
DisableProgramGroupPage=yes
DisableDirPage=auto
; For this person alone unless they choose everyone; an update keeps what was chosen.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
WizardStyle=modern
SetupIconFile=..\apps\openmailclient\assets\openmailclient.ico
UninstallDisplayIcon={app}\openmailclient.exe
UninstallDisplayName=OpenMailClient
LicenseFile=..\LICENSE-MIT
OutputDir=..\target\dist
OutputBaseFilename=OpenMailClient-{#AppVersion}-windows-x64-setup
Compression=lzma2/max
SolidCompression=yes
; The running program is closed first; OpenMailClient starts itself again (RELAUNCH below).
CloseApplications=yes
RestartApplications=no
ChangesAssociations=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "startmenu"; Description: "Put OpenMailClient on the &Start menu"; GroupDescription: "Shortcuts:"
Name: "desktopicon"; Description: "Put an icon on the &desktop"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "{#SourceDir}\openmailclient.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\openmailclient-cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\ATTRIBUTION.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\OpenMailClient"; Filename: "{app}\openmailclient.exe"; Tasks: startmenu
Name: "{autodesktop}\OpenMailClient"; Filename: "{app}\openmailclient.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\openmailclient.exe"; Description: "Start OpenMailClient"; Flags: nowait postinstall skipifsilent
; An update asked for in the program starts it again once it is installed.
Filename: "{app}\openmailclient.exe"; Flags: nowait; Check: Relaunch

[UninstallDelete]
; Installers brought down by Check for Updates; mail and settings are elsewhere and stay.
Type: filesandordirs; Name: "{localappdata}\OpenMailClient\Updates"

[Code]
function Relaunch: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:RELAUNCH|0}') = '1');
end;
