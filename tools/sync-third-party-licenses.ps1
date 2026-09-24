param(
    [string]$Destination = ""
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $Destination) {
    $Destination = Join-Path $projectRoot "licenses"
}
New-Item -ItemType Directory -Path $Destination -Force | Out-Null

$artifacts = @(
    @{
        Name = "Qwen3-4B-Apache-2.0.txt"
        Url = "https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/bc640142c66e1fdd12af0bd68f40445458f3869b/LICENSE"
        Sha256 = "5de36594c10839788a8c589443a8ef9d8b8d17c65a1b5807206ae037fc36c6bd"
    },
    @{
        Name = "llama.cpp-MIT.txt"
        Url = "https://raw.githubusercontent.com/ggml-org/llama.cpp/b29c606e2/LICENSE"
        Sha256 = "94f29bbed6a22c35b992c5c6ebf0e7c92f13b836b90f36f461c9cf2f0f1d010d"
    },
    @{
        Name = "ONNX-Runtime-MIT.txt"
        Url = "https://raw.githubusercontent.com/microsoft/onnxruntime/2d92497/LICENSE"
        Sha256 = "2f07c72751aed99790b8a4869cf2311df85a860b22ded05fa22803587a48922c"
    },
    @{
        Name = "ONNX-Runtime-ThirdPartyNotices.txt"
        Url = "https://raw.githubusercontent.com/microsoft/onnxruntime/2d92497/ThirdPartyNotices.txt"
        Sha256 = "0e07b95f3a8d6230037707c5c4a2b554d12c4cb67369669ac255635528ffcee2"
    },
    @{
        Name = "NVIDIA-CUDA-EULA-2026-01-26.html"
        Url = "https://docs.nvidia.com/cuda/eula/index.html"
        Sha256 = "dae4884999a096f1e61e976336344e95a3705eca1a87caf519c3f70b6d859a06"
    }
)

foreach ($artifact in $artifacts) {
    $target = Join-Path $Destination $artifact.Name
    $temporary = "$target.download"
    try {
        Invoke-WebRequest -Uri $artifact.Url -OutFile $temporary -UseBasicParsing -Headers @{
            "User-Agent" = "ILIA-license-sync"
        }
        $actual = (Get-FileHash -LiteralPath $temporary -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $artifact.Sha256) {
            throw "License hash mismatch for $($artifact.Name): expected $($artifact.Sha256), got $actual"
        }
        Move-Item -LiteralPath $temporary -Destination $target -Force
    } finally {
        if (Test-Path -LiteralPath $temporary) {
            Remove-Item -LiteralPath $temporary -Force
        }
    }
}

$artifacts | ForEach-Object {
    [ordered]@{
        file = $_.Name
        source = $_.Url
        sha256 = $_.Sha256
    }
} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $Destination "downloaded-license-manifest.json") -Encoding utf8

Write-Host "Synchronized and verified $($artifacts.Count) third-party license artifacts in $Destination"
