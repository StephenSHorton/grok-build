#Requires -Version 5.1
<#
.SYNOPSIS
  One-shot new-machine setup for this personal fork (Windows).

.DESCRIPTION
  Ensures upstream remote, cargo/dotslash, runs install-local.ps1, verifies
  daily driver is grok.cmd -> scripts/grok.ps1 (not a frozen .exe).
#>
$ErrorActionPreference = 'Stop'

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $RepoRoot

function Die([string]$Message) { Write-Error "error: $Message"; exit 1 }
function Ok([string]$Message) { Write-Host "ok: $Message" }

Write-Host '== Grok fork daily-driver setup (Windows) =='
Write-Host "repo: $RepoRoot"
Write-Host ''

if (-not (Test-Path (Join-Path $RepoRoot '.git'))) {
    Die "not a git checkout: $RepoRoot"
}

$remotes = & git remote
if ($remotes -notcontains 'origin') {
    Die "missing remote 'origin' (clone your fork)"
}
if ($remotes -notcontains 'upstream') {
    Write-Host 'adding upstream -> https://github.com/xai-org/grok-build.git'
    & git remote add upstream https://github.com/xai-org/grok-build.git
}
Ok "origin=$(& git remote get-url origin)"
Ok "upstream=$(& git remote get-url upstream)"
& git fetch --quiet upstream main 2>$null
if ($LASTEXITCODE -ne 0) {
    Write-Host 'warning: could not fetch upstream (offline? continuing)'
}
Write-Host ''

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Die 'cargo not found - install Rust from https://rustup.rs then re-run'
}
Ok "cargo=$(Get-Command cargo | Select-Object -ExpandProperty Source)"

if (-not (Get-Command dotslash -ErrorAction SilentlyContinue)) {
    Write-Host 'installing dotslash...'
    & cargo install dotslash
    if ($LASTEXITCODE -ne 0) { Die 'cargo install dotslash failed' }
}
Ok "dotslash present"

$grokHome = if ($env:GROK_HOME) { $env:GROK_HOME } else { Join-Path $HOME '.grok' }
if (-not (Test-Path $grokHome)) {
    Write-Host "warning: $grokHome missing - install stock Grok from https://x.ai/cli first"
}

Write-Host 'running install-local.ps1 (seed release build; can take a long time on Windows)...'
& (Join-Path $RepoRoot 'scripts\install-local.ps1')
if ($LASTEXITCODE -ne 0) { Die 'install-local.ps1 failed' }

$cmd = Join-Path $grokHome 'bin\grok.cmd'
if (-not (Test-Path $cmd)) { Die "missing $cmd after install" }
if (Test-Path (Join-Path $grokHome 'bin\grok.exe')) {
    Die 'grok.exe still in bin/ - would shadow grok.cmd. Re-run install-local.ps1'
}
Ok "daily driver -> $cmd"
Write-Host ''
Write-Host '== setup complete =='
Write-Host 'Every open of grok fetches origin + upstream and rebuilds at most once if HEAD moved.'
Write-Host 'Escape hatch (stock): grok-official'
Write-Host "Details: $RepoRoot\AGENTS.md"
