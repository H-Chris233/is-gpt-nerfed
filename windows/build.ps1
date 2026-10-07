$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    $frontend = Join-Path $projectRoot 'frontend'
    npm.cmd --prefix "$frontend" ci --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw 'Frontend dependency installation failed' }
    npm.cmd --prefix "$frontend" run build
    if ($LASTEXITCODE -ne 0) { throw 'Frontend typecheck or build failed' }
    cargo build --locked --release --bin is-gpt-nerfed
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed' }
    $destination = Join-Path $projectRoot 'dist/IsGPTNerfed-Windows-x64.exe'
    New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot 'target/release/is-gpt-nerfed.exe') -Destination $destination
    $hash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath "$destination.sha256" -Value "$hash  IsGPTNerfed-Windows-x64.exe" -Encoding ascii
    Write-Output $destination
    Write-Output "SHA256: $hash"
} finally {
    Pop-Location
}
