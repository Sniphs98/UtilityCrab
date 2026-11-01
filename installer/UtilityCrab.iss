; Inno-Setup-Skript für UtilityCrab.
; Bauen: installer\build.ps1 (oder vorher "cargo build --release" und dann ISCC.exe installer\UtilityCrab.iss)

#define AppName "UtilityCrab"
#define AppExe "utility_crab.exe"
; Wird von build.ps1 per /DAppVersion=... aus Cargo.toml übergeben.
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define Release "..\target\release"
; ISCC ist 32-Bit: "Sysnative" zeigt auf das echte 64-Bit-System32.
#define SysDir GetEnv("windir") + "\Sysnative"

[Setup]
AppId={{4009F49D-F37D-422A-8D34-878398349860}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Sniphs
; Installation pro Benutzer, keine Admin-Rechte nötig.
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\{#AppName}
DisableProgramGroupPage=yes
DisableDirPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename={#AppName}-Setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
UninstallDisplayIcon={app}\{#AppExe}
CloseApplications=force
RestartApplications=no
WizardStyle=modern

[Languages]
Name: "de"; MessagesFile: "compiler:Languages\German.isl"

[Tasks]
Name: "autostart"; Description: "UtilityCrab beim Windows-Start automatisch starten"
Name: "desktopicon"; Description: "Desktop-Verknüpfung erstellen"; Flags: unchecked

[Files]
Source: "{#Release}\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Release}\DirectML.dll"; DestDir: "{app}"; Flags: ignoreversion
; Visual-C++-Runtime app-lokal mitliefern, damit kein separates Redistributable nötig ist.
Source: "{#SysDir}\msvcp140.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SysDir}\msvcp140_1.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SysDir}\vcruntime140.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SysDir}\vcruntime140_1.dll"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; \
    ValueName: "{#AppName}"; ValueData: """{app}\{#AppExe}"""; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\{#AppExe}"; Description: "UtilityCrab jetzt starten"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "taskkill.exe"; Parameters: "/F /IM {#AppExe}"; Flags: runhidden; RunOnceId: "KillApp"

[Code]
procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
begin
  // Läuft das Tool noch (Tray), vor dem Kopieren beenden, sonst ist die exe gesperrt.
  if CurStep = ssInstall then
    Exec('taskkill.exe', '/F /IM {#AppExe}', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  // Autostart-Eintrag entfernen, falls die Option bei einem Update abgewählt wurde.
  if (CurStep = ssPostInstall) and not WizardIsTaskSelected('autostart') then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#AppName}');
end;
