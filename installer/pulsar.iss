; Compiled by scripts\package.ps1, which passes AppVersion and SourceDir.
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\dist\stage"
#endif
#define AppGuid "8F3B6C1E-3D8A-4B8E-9C61-5A2E7D4F0B19"
#define RunKey "Software\Microsoft\Windows\CurrentVersion\Run"

[Setup]
AppId={{{#AppGuid}}
AppName=Pulsar
AppVersion={#AppVersion}
AppVerName=Pulsar {#AppVersion}
AppPublisher=oMaN-Rod
AppPublisherURL=https://github.com/oMaN-Rod/Pulsar
AppSupportURL=https://github.com/oMaN-Rod/Pulsar/issues
AppUpdatesURL=https://github.com/oMaN-Rod/Pulsar/releases
VersionInfoVersion={#AppVersion}
DefaultDirName={localappdata}\Programs\Pulsar
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.22000
OutputDir=..\dist
OutputBaseFilename=pulsar-{#AppVersion}-setup
SetupIconFile=..\crates\pulsar-monitor\assets\pulsar.ico
UninstallDisplayIcon={app}\pulsar.exe
UninstallDisplayName=Pulsar
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
CloseApplications=no

[Tasks]
Name: autostart; Description: "Start Pulsar when I sign in"; Check: not IsUpgrade

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Pulsar"; Filename: "{app}\pulsar.exe"

[Registry]
Root: HKCU; Subkey: "{#RunKey}"; ValueType: string; ValueName: "Pulsar"; ValueData: """{app}\pulsar.exe"""; Tasks: autostart

[Run]
Filename: "{app}\pulsar.exe"; Description: "Start Pulsar now"; Flags: nowait postinstall skipifsilent
Filename: "{app}\pulsar.exe"; Flags: nowait; Check: RestartAfterSilentUpgrade

[Code]
const
  WM_CLOSE = $0010;

var
  WasRunning: Boolean;

function IsUpgrade: Boolean;
begin
  Result := RegValueExists(HKCU,
    'Software\Microsoft\Windows\CurrentVersion\Uninstall\{' + '{#AppGuid}' + '}_is1',
    'UninstallString');
end;

function AppWindow: HWND;
begin
  Result := FindWindowByClassName('PulsarHost');
end;

function SettingsWindow: HWND;
begin
  Result := FindWindowByWindowName('Pulsar Settings');
end;

{ Asks both processes to close, so Settings saves pending edits, then forces them. }
procedure ClosePulsar;
var
  I, ResultCode: Integer;
begin
  if SettingsWindow <> 0 then
    PostMessage(SettingsWindow, WM_CLOSE, 0, 0);
  if AppWindow <> 0 then
    PostMessage(AppWindow, WM_CLOSE, 0, 0);
  for I := 1 to 50 do
  begin
    if (AppWindow = 0) and (SettingsWindow = 0) then
      Break;
    Sleep(100);
  end;
  Sleep(300);
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM pulsar.exe /IM pulsar-settings.exe',
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  WasRunning := AppWindow <> 0;
  ClosePulsar;
  Result := '';
end;

function RestartAfterSilentUpgrade: Boolean;
begin
  Result := WizardSilent and WasRunning;
end;

function InitializeUninstall: Boolean;
begin
  ClosePulsar;
  Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    RegDeleteValue(HKCU, '{#RunKey}', 'Pulsar');
    if not UninstallSilent and
      (MsgBox('Also remove your Pulsar settings and logs?', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
    begin
      DelTree(ExpandConstant('{userappdata}\Pulsar'), True, True, True);
      DelTree(ExpandConstant('{localappdata}\Pulsar'), True, True, True);
    end;
  end;
end;
