param(
    [string]$MakeNsis = 'makensis.exe',
    [string]$Output = 'dist/PocketCam-Setup.exe'
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $root
$staged = $null
try {
    cargo build --workspace --release --locked
    if ($LASTEXITCODE) { throw 'PocketCam build failed.' }
    $destination = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Output)
    $folder = [IO.Path]::GetDirectoryName($destination)
    [void][IO.Directory]::CreateDirectory($folder)
    # Each compiler writes its own file. Replace the public file only after verification.
    $staged = Join-Path $folder ("PocketCam-Setup-{0}.partial.exe" -f [Guid]::NewGuid())
    & $MakeNsis "/DROOT=$root" "/DOUTPUT=$staged" "$root\scripts\PocketCam.nsi"
    if ($LASTEXITCODE) { throw 'Installer build failed.' }
    & "$PSScriptRoot/Verify-Setup.ps1" $staged
    try { [IO.File]::Move($staged, $destination) }
    catch [IO.IOException] {
        if (![IO.File]::Exists($destination)) { throw }
        [IO.File]::Replace($staged, $destination, [NullString]::Value)
    }
    Get-FileHash -LiteralPath $destination -Algorithm SHA256
} finally {
    if ($staged -and [IO.File]::Exists($staged)) { [IO.File]::Delete($staged) }
    Pop-Location
}
