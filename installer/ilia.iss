#define AppName "ILIA"
#ifndef AppVersion
  #define AppVersion "1.0.1"
#endif
#ifndef AppFileVersion
  #define AppFileVersion "1.0.1.0"
#endif
#ifndef SourceDir
  #error SourceDir must point to the staged ILIA application directory
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif
#ifndef WebView2Installer
  #define WebView2Installer ""
#endif
#ifndef VCRedistInstaller
  #error VCRedistInstaller must point to the official Microsoft x64 redistributable
#endif

[Setup]
AppId={{A1A60D67-CA13-4A51-93E7-445D2A7CF031}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=ILIA
DefaultDirName={localappdata}\Programs\ILIA
DefaultGroupName=ILIA
DisableProgramGroupPage=yes
OutputDir={#OutputDir}
OutputBaseFilename=ILIA-{#AppVersion}-windows-x64-offline-setup
SetupIconFile={#SourceDir}\icon.ico
UninstallDisplayIcon={app}\ilia-desktop.exe
; Quantized GGUF/ONNX weights are already dense. Disabling recompression keeps
; reproducible offline builds and installs fast without materially shrinking them.
Compression=none
SolidCompression=no
DiskSpanning=yes
DiskSliceSize=1900000000
SlicesPerDisk=1
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
MinVersion=10.0.17763
VersionInfoVersion={#AppFileVersion}
VersionInfoProductName={#AppName}
LicenseFile={#SourceDir}\licenses\INSTALLER-THIRD-PARTY-TERMS.txt

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#VCRedistInstaller}"; DestDir: "{tmp}"; Flags: deleteafterinstall
#if WebView2Installer != ""
Source: "{#WebView2Installer}"; DestDir: "{tmp}"; Flags: deleteafterinstall
#endif

[Icons]
Name: "{autoprograms}\ILIA"; Filename: "{app}\ilia-desktop.exe"
Name: "{autodesktop}\ILIA"; Filename: "{app}\ilia-desktop.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "快捷方式："

[Run]
Filename: "{tmp}\VC_redist.x64.exe"; Parameters: "/install /quiet /norestart"; StatusMsg: "正在安装 Microsoft Visual C++ Runtime..."; Flags: waituntilterminated runhidden; Check: not IsRequiredVCRuntimeInstalled
#if WebView2Installer != ""
Filename: "{tmp}\MicrosoftEdgeWebView2RuntimeInstallerX64.exe"; Parameters: "/silent /install"; StatusMsg: "正在安装 Microsoft Edge WebView2 Runtime..."; Flags: waituntilterminated runhidden; Check: not IsWebView2Installed
#endif
Filename: "{app}\ilia-desktop.exe"; Description: "启动 ILIA"; Flags: nowait postinstall skipifsilent

[Code]
function IsRequiredVCRuntimeInstalled: Boolean;
var
  Installed, Major, Minor, Build: Cardinal;
  Key: String;
begin
  Key := 'SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64';
  Result :=
    RegQueryDWordValue(HKLM64, Key, 'Installed', Installed) and
    RegQueryDWordValue(HKLM64, Key, 'Major', Major) and
    RegQueryDWordValue(HKLM64, Key, 'Minor', Minor) and
    RegQueryDWordValue(HKLM64, Key, 'Bld', Build) and
    (Installed = 1) and
    ((Major > 14) or ((Major = 14) and ((Minor > 51) or
      ((Minor = 51) and (Build >= 36247)))));
end;

procedure StopIliaProcesses;
var
  ResultCode: Integer;
  Command: String;
begin
  { llama-server is a child process of the desktop app.  If Windows closes the
    desktop process before its shutdown handler runs, the child can survive and
    keep CUDA/Vulkan DLLs locked.  Restrict termination to executables below
    this installation directory so other llama.cpp installations are untouched. }
  Command :=
    '-NoProfile -NonInteractive -WindowStyle Hidden -Command ' +
    '"& { $app = ''' + ExpandConstant('{app}') + '''; ' +
    '$names = @(''ilia-desktop.exe'', ''llama-server.exe'', ''ilia-updater.exe''); ' +
    'Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | ' +
    'Where-Object { $_.ExecutablePath -and ' +
    '$_.ExecutablePath.StartsWith($app, [System.StringComparison]::OrdinalIgnoreCase) -and ' +
    '$_.Name -in $names } | ' +
    'ForEach-Object { Invoke-CimMethod -InputObject $_ -MethodName Terminate | Out-Null } }"';
  Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'), Command,
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Sleep(500);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then begin
    StopIliaProcesses;
  end;
  if CurUninstallStep = usPostUninstall then begin
    { Remove an otherwise-empty installation directory after unlocked files have
      been processed by the normal uninstaller. }
    DelTree(ExpandConstant('{app}'), True, True, True);
  end;
end;

function HasWebView2Version(RootKey: Integer; SubKey: String): Boolean;
var
  Version: String;
begin
  Result := RegQueryStringValue(RootKey, SubKey, 'pv', Version) and
    (Version <> '') and (Version <> '0.0.0.0');
end;

function IsWebView2Installed: Boolean;
begin
  Result := HasWebView2Version(HKCU,
    'Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}') or
    HasWebView2Version(HKLM,
    'Software\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}');
end;
