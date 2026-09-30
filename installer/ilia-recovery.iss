#define AppName "ILIA"
#ifndef AppVersion
  #define AppVersion "1.1.10"
#endif
#ifndef AppFileVersion
  #define AppFileVersion "1.1.10.0"
#endif
#ifndef UpdatePackage
  #error UpdatePackage must point to a signed .ilia update package
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

[Setup]
AppId={{A1A60D67-CA13-4A51-93E7-445D2A7CF031}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=ILIA
DefaultDirName={code:GetInstallDir}
DefaultGroupName=ILIA
DisableProgramGroupPage=yes
DisableDirPage=auto
OutputDir={#OutputDir}
OutputBaseFilename=ILIA-{#AppVersion}-windows-x64-recovery
Compression=lzma2
SolidCompression=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
Uninstallable=no
MinVersion=10.0.17763
VersionInfoVersion={#AppFileVersion}
VersionInfoProductName={#AppName} Recovery Update

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "{#UpdatePackage}"; DestDir: "{app}\.ilia-update"; DestName: "recovery-{#AppVersion}.ilia"; Flags: deleteafterinstall

[Icons]
Name: "{autoprograms}\ILIA"; Filename: "{app}\ilia-desktop.exe"
Name: "{autodesktop}\ILIA"; Filename: "{app}\ilia-desktop.exe"; Comment: "ILIA 国际法智能助手"

[Code]
function GetInstallDir(Param: String): String;
var
  Candidate: String;
  DriveCode: Integer;
begin
  Candidate := ExpandConstant('{localappdata}\Programs\ILIA');
  if FileExists(Candidate + '\ilia-desktop.exe') then begin
    Result := Candidate;
    exit;
  end;
  for DriveCode := Ord('C') to Ord('Z') do begin
    Candidate := Chr(DriveCode) + ':\ILIA';
    if FileExists(Candidate + '\ilia-desktop.exe') then begin
      Result := Candidate;
      exit;
    end;
  end;
  Result := ExpandConstant('{localappdata}\Programs\ILIA');
end;

function RequiredFilesPresent: Boolean;
begin
  Result := FileExists(ExpandConstant('{app}\ilia-updater.exe')) and
    FileExists(ExpandConstant('{app}\update\trusted-key.json'));
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  if not RequiredFilesPresent then begin
    Result := '未找到现有 ILIA 安装。请先安装 1.1.7 完整离线版，再运行此恢复更新。';
  end else
    Result := '';
end;

procedure StopIliaProcesses;
var
  ResultCode: Integer;
  Command: String;
begin
  Command :=
    '-NoProfile -NonInteractive -WindowStyle Hidden -Command ' +
    '"& { $app = ''' + ExpandConstant('{app}') + '''; ' +
    '$names = @(''ilia-desktop.exe'', ''llama-server.exe''); ' +
    'Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | ' +
    'Where-Object { $_.ExecutablePath -and ' +
    '$_.ExecutablePath.StartsWith($app, [System.StringComparison]::OrdinalIgnoreCase) -and ' +
    '$_.Name -in $names } | ' +
    'ForEach-Object { Invoke-CimMethod -InputObject $_ -MethodName Terminate | Out-Null } }"';
  Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'), Command,
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Sleep(500);
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
  Parameters: String;
begin
  if CurStep = ssInstall then begin
    StopIliaProcesses;
  end;
  if CurStep = ssPostInstall then begin
    WizardForm.StatusLabel.Caption := '正在验证并修复 ILIA...';
    Parameters :=
      'apply-package --root "' + ExpandConstant('{app}') + '" ' +
      '--package "' + ExpandConstant('{app}\.ilia-update\recovery-{#AppVersion}.ilia') + '" ' +
      '--public-key "' + ExpandConstant('{app}\update\trusted-key.json') + '"';
    if not Exec(ExpandConstant('{app}\ilia-updater.exe'), Parameters, '',
      SW_HIDE, ewWaitUntilTerminated, ResultCode) then
      RaiseException('无法启动 ILIA 更新器。');
    if ResultCode <> 0 then
      RaiseException('签名更新未能完成。现有版本已保持或回滚，错误代码：' + IntToStr(ResultCode));
    if not ShellExec('', ExpandConstant('{app}\ilia-desktop.exe'), '',
      ExpandConstant('{app}'), SW_SHOWNORMAL, ewNoWait, ResultCode) then
      RaiseException('更新已经完成，但无法自动打开 ILIA。请使用桌面快捷方式启动。');
  end;
end;
