; Per-user installer, so neither installing nor updating asks for an
; administrator. Build with scripts/package-windows.ps1.

#ifndef AppVersion
  #error Pass the version with /DAppVersion=X.Y.Z
#endif
#ifndef AppExe
  #error Pass the built executable with /DAppExe=path
#endif

[Setup]
; The installer finds earlier installs by this ID. Never change it.
AppId={{010A8C7A-5F34-45F6-ACF0-27453429DEB9}
AppName=Request Eagle
AppVersion={#AppVersion}
AppVerName=Request Eagle {#AppVersion}
AppPublisher=Gregor Tokarev
AppPublisherURL=https://requesteagle.tokarev.work
AppSupportURL=https://github.com/gregor-tokarev/request-eagle/issues
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\Request Eagle
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputBaseFilename=RequestEagle-{#AppVersion}-x64-setup
SetupIconFile=request-eagle.ico
UninstallDisplayIcon={app}\request-eagle.exe
UninstallDisplayName=Request Eagle
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes

[Tasks]
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#AppExe}"; DestDir: "{app}"; DestName: "request-eagle.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Request Eagle"; Filename: "{app}\request-eagle.exe"
Name: "{autodesktop}\Request Eagle"; Filename: "{app}\request-eagle.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\request-eagle.exe"; Description: "{cm:LaunchProgram,Request Eagle}"; Flags: nowait postinstall skipifsilent
