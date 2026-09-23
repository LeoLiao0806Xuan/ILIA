#define AppName "ILIA"
#ifndef AppVersion
  #define AppVersion "1.0.0-rc.1"
#endif
#ifndef AppFileVersion
  #define AppFileVersion "1.0.0.1"
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

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
#if WebView2Installer != ""
Source: "{#WebView2Installer}"; DestDir: "{tmp}"; Flags: deleteafterinstall
#endif

[Icons]
Name: "{autoprograms}\ILIA"; Filename: "{app}\ilia-desktop.exe"
Name: "{autodesktop}\ILIA"; Filename: "{app}\ilia-desktop.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "快捷方式："

[Run]
#if WebView2Installer != ""
Filename: "{tmp}\MicrosoftEdgeWebView2RuntimeInstallerX64.exe"; Parameters: "/silent /install"; StatusMsg: "正在安装 Microsoft Edge WebView2 Runtime..."; Flags: waituntilterminated runhidden; Check: not IsWebView2Installed
#endif
Filename: "{app}\ilia-desktop.exe"; Description: "启动 ILIA"; Flags: nowait postinstall skipifsilent

[Code]
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
