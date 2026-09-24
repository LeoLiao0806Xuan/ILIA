# Microsoft runtime redistribution record

## Visual C++ runtime

The next-release installer embeds Microsoft's official signed x64
Redistributable package instead of copying DLLs from `System32` or requiring a
local Visual Studio C++ workload:

- Source: <https://aka.ms/vc14/vc_redist.x64.exe>
- Version: `14.51.36247.0`
- SHA-256: `843068991daaa1f73ad9f6239bce4d0f6a07a51f18c37ea2a867e9beca71295c`
- Authenticode signer: Microsoft Corporation

The build verifies size, hash and signer against
`licenses/microsoft-redistributables.json`; Inno Setup runs the unmodified
package with `/install /quiet /norestart` before launching ILIA.

ILIA deploys unmodified x64 copies of:

* `msvcp140.dll`
* `vcruntime140.dll`
* `vcruntime140_1.dll`

Microsoft's Visual Studio redistribution list permits licensed Visual Studio
users to distribute the unmodified files from the applicable `VC\Redist`
directory with their programs. The build owner is responsible for using a
validly licensed Visual Studio/Build Tools installation and an allowed release
source:

* <https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files>
* <https://learn.microsoft.com/visualstudio/releases/2022/redistribution>

The installer build accepts these files only from `VCToolsRedistDir` (or an
explicit `-VCRuntimeDirectory`) under `x64\Microsoft.VC143.CRT`; it no longer
copies DLLs from Windows `System32`. The package manifest records the exact
hashes of the copies staged by each build. No separate Microsoft application is
normally required, but the right depends on satisfying the applicable Visual
Studio license terms.

## Microsoft Edge WebView2 Runtime

ILIA includes the unmodified Evergreen Standalone Installer for offline x64
deployment and runs it only when WebView2 is absent. Microsoft documents this
as a supported offline distribution workflow:

<https://learn.microsoft.com/microsoft-edge/webview2/concepts/distribution#offline-deployment>

Recorded ILIA 1.0.0 prerequisite:

* file: `MicrosoftEdgeWebView2RuntimeInstallerX64.exe`
* file version: `1.3.269.9`
* SHA-256: `ad9b350625e132481bc0953eee9e032810134df9fedbd7be364c3f4e0e4dbd64`

The release owner must refresh this record when the installer changes.
