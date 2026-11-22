param([Parameter(Mandatory)][string]$Tag)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-Location (Join-Path $PSScriptRoot '..')
$repository = 'Threalcocoboy/WinX'
if ($env:GITHUB_REPOSITORY -cne $repository) { throw "Publication requires the $repository Actions repository." }
if ($Tag -cnotmatch '^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$') {
    throw "Invalid WinX release tag $Tag."
}
$version = $Tag.Substring(1)
$preview = $version.Contains('-')
$notes = Join-Path 'docs/releases' "$Tag.md"
if (-not (Test-Path -LiteralPath $notes -PathType Leaf)) { throw "Missing release notes $notes." }

python scripts/verify-windows-package.py $version
$assetNames = @("WinX-$Tag-Setup-x64.exe", "WinX-$Tag-Portable-x64.zip", "WinX-$Tag-Source.zip", 'SHA256SUMS.txt')
$assetPaths = @($assetNames | ForEach-Object { Join-Path 'dist' $_ })

function Get-WinXRelease {
    $pages = gh api "repos/$repository/releases?per_page=100" --paginate --slurp | ConvertFrom-Json -NoEnumerate
    foreach ($page in $pages) {
        foreach ($release in $page) {
            if ($release.tag_name -ceq $Tag) { return $release }
        }
    }
}

function Assert-WinXAssets($release) {
    if ($release.prerelease -ne $preview -or $release.assets.Count -ne $assetNames.Count) {
        throw 'GitHub release channel or asset count does not match the verified packages.'
    }
    foreach ($name in $assetNames) {
        $matches = @($release.assets | Where-Object { $_.name -ceq $name })
        $path = Join-Path 'dist' $name
        $asset = $matches[0]
        $digest = 'sha256:' + (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
        if ($matches.Count -ne 1 -or $asset.state -ne 'uploaded' -or
            $asset.size -ne (Get-Item -LiteralPath $path).Length -or $asset.digest -cne $digest) {
            throw "GitHub asset $name differs from the verified local file."
        }
    }
}

$release = Get-WinXRelease
if ($release -and -not $release.draft) {
    Assert-WinXAssets $release
    Write-Output "Verified existing $($release.html_url)"
    return
}

$title = if ($Tag -ceq 'v1.5.0') { "WinX $Tag — Initial Public Release" } else { "WinX $Tag" }
if (-not $release) {
    # A release branch can create its tag only after the package job passes.
    if ($env:GITHUB_REF -ceq "refs/heads/release/$Tag") {
        $PSNativeCommandUseErrorActionPreference = $false
        $existing = gh api "repos/$repository/git/ref/tags/$Tag" --jq '.object.sha' 2>$null
        $lookupExit = $LASTEXITCODE
        $PSNativeCommandUseErrorActionPreference = $true
        if ($lookupExit -ne 0) {
            gh api --method POST "repos/$repository/git/refs" -f "ref=refs/tags/$Tag" -f "sha=$env:GITHUB_SHA" | Out-Null
        } elseif ($existing -cne $env:GITHUB_SHA) {
            throw "Tag $Tag already points to a different commit."
        }
    }
    $create = @('release', 'create', $Tag, '--repo', $repository, '--verify-tag',
        '--draft', '--title', $title, '--notes-file', $notes)
    if ($preview) { $create += '--prerelease' }
    gh @create
}
gh release upload $Tag @assetPaths --repo $repository --clobber
$release = Get-WinXRelease
if (-not $release.draft) { throw 'The expected draft release could not be verified.' }
Assert-WinXAssets $release
$latest = if ($preview) { '--latest=false' } else { '--latest=true' }
gh release edit $Tag --repo $repository --draft=false $latest
$published = Get-WinXRelease
if (-not $published -or $published.draft) { throw 'GitHub did not confirm publication.' }
Assert-WinXAssets $published
Write-Output "Verified public $($published.html_url)"
