param([string]$ReleaseTag)

$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
$metadata = cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'Could not read the release version' }
$version = ($metadata.packages | Where-Object Name -eq 'nothingx-desktop').version
if ($version -notmatch '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$') {
    throw 'WinX requires a SemVer release version without build metadata.'
}
if ($ReleaseTag -and $ReleaseTag -cne "v$version") {
    throw "Release tag $ReleaseTag must match Cargo version v$version."
}
cargo build --release --locked --target x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw 'Rust build failed' }
python scripts/package-source.py --windows
if ($LASTEXITCODE -ne 0) { throw 'Source packaging failed' }
python scripts/package-windows.py target/x86_64-pc-windows-msvc/release/nothingx-desktop.exe --installer --local-names
if ($LASTEXITCODE -ne 0) { throw 'Windows packaging failed' }
$artifacts = @("WinX-v$version-Setup-x64.exe", "WinX-v$version-Portable-x64.zip", "WinX-v$version-Source.zip")
$checksums = foreach ($name in $artifacts) {
    $hash = Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path dist $name)
    '{0}  {1}' -f $hash.Hash.ToLowerInvariant(), $name
}
$checksums | Set-Content -Encoding ascii -LiteralPath dist/SHA256SUMS.txt
$checksums
python scripts/verify-windows-package.py $version
if ($LASTEXITCODE -ne 0) { throw 'Windows package verification failed' }
