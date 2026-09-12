$ErrorActionPreference = "Stop"

$cargoToml = Get-Content Cargo.toml
$versionMatch = $cargoToml | Select-String -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $versionMatch) {
    Write-Host "Error: version not found in Cargo.toml"
    exit 1
}
$version = $versionMatch.Matches[0].Groups[1].Value

$tag = "v$version"
Write-Host "Version: $version"
Write-Host "Tag: $tag"

$existing = git tag --list $tag
if ($existing) {
    Write-Host "Error: tag $tag already exists. Bump the version in Cargo.toml first."
    exit 1
}

$confirm = Read-Host "Proceed? (y/N)"
if ($confirm -ne 'y') {
    Write-Host "Aborted."
    exit 1
}

git tag -a $tag -m "Release $tag"
git push origin $tag

Write-Host "Done. Tag $tag pushed."
