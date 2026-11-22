$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-Location (Join-Path $PSScriptRoot '..')

cargo +1.95.0 fmt --all --check
cargo +1.95.0 clippy --locked --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cargo +1.95.0 build --locked --target x86_64-pc-windows-msvc
cargo +1.95.0 test --locked --target x86_64-pc-windows-msvc
node scripts/check-ui.cjs
node scripts/check-settings.cjs
node scripts/check-onboarding.cjs
node --check ui/src/popover/winx-settings.js

$tokens = $null
$errors = $null
[void][System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path 'src/update-install.ps1').Path, [ref]$tokens, [ref]$errors)
if ($errors) { throw ($errors -join [Environment]::NewLine) }
