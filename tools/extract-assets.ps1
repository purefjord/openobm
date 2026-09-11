# Extract the original MIDlet's resources into ./assets for OpenOBM.
#
#   pwsh tools/extract-assets.ps1 path\to\Oblivion.jar
#
# A .jar is a zip; the game's resources sit at its root. We take everything
# except the Java classes and the manifest.
param(
    [Parameter(Mandatory = $true)][string]$Jar,
    [string]$Dest = "assets"
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path -LiteralPath $Jar)) { throw "No such file: $Jar" }

$staging = Join-Path ([System.IO.Path]::GetTempPath()) ("openobm-" + [guid]::NewGuid())
$zip = Join-Path $staging "midlet.zip"
New-Item -ItemType Directory -Path $staging -Force | Out-Null
Copy-Item -LiteralPath $Jar -Destination $zip
Expand-Archive -LiteralPath $zip -DestinationPath $staging -Force

New-Item -ItemType Directory -Path $Dest -Force | Out-Null
$kept = 0
Get-ChildItem -LiteralPath $staging -File -Recurse |
    Where-Object { $_.Extension -notin '.class', '.zip', '.jar', '.jad' -and $_.FullName -notlike '*META-INF*' } |
    ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $Dest $_.Name) -Force
        $kept++
    }
Remove-Item -LiteralPath $staging -Recurse -Force

Write-Host "Extracted $kept resource files to $Dest/"
Write-Host "Parser checks: cargo test -p eso-tools --features assets --locked"
Write-Host "Play: cargo run -p game --features interactive --bin play --release --locked"
