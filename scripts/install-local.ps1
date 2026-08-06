#Requires -Version 5.1
<#
.SYNOPSIS
  Wire the Windows fork launcher as the daily driver under ~/.grok/bin.

.DESCRIPTION
  Daily driver must be grok.cmd -> scripts/grok.ps1 (not a frozen grok.exe).
  Stock grok.exe is moved aside so PATHEXT does not prefer .exe over .cmd.
  Always refreshes grok-official.cmd -> newest stock download / backup.

.PARAMETER SkipBuild
  Do not cargo build (first launch will build if needed).

.PARAMETER Rollback
  Restore stock grok.exe as the default (remove launcher cmd).

.PARAMETER EnsureOfficial
  Only install/update grok-official.cmd.

.PARAMETER KeepAutoUpdate
  Do not set auto_update = false in config.toml.
#>
param(
    [switch]$SkipBuild,
    [switch]$Rollback,
    [switch]$EnsureOfficial,
    [switch]$KeepAutoUpdate
)

$ErrorActionPreference = 'Stop'

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$GrokHome = if ($env:GROK_HOME) { $env:GROK_HOME } else { Join-Path $HOME '.grok' }
$BinDir = Join-Path $GrokHome 'bin'
$Downloads = Join-Path $GrokHome 'downloads'
$LauncherPs1 = Join-Path $RepoRoot 'scripts\grok.ps1'
$Binary = Join-Path $RepoRoot 'target\release\xai-grok-pager.exe'
$Stamp = Join-Path $RepoRoot '.fork-built-at'
$Package = 'xai-grok-pager-bin'

function Write-Info([string]$Message) { Write-Host $Message }
function Die([string]$Message) { Write-Error "error: $Message"; exit 1 }

function Ensure-Layout {
    New-Item -ItemType Directory -Force -Path $BinDir, $Downloads | Out-Null
}

function Get-NewestStockExe {
    $candidates = @()
    foreach ($name in @('grok-windows-amd64.exe', 'grok.exe')) {
        $p = Join-Path $Downloads $name
        if (Test-Path $p) { $candidates += Get-Item $p }
    }
    $versioned = Get-ChildItem -Path $Downloads -Filter 'grok-*-windows-*.exe' -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -notmatch 'fork|staging' }
    if ($versioned) { $candidates += $versioned }
    # Also accept a backup left in bin/
    $backup = Join-Path $BinDir 'grok.stock.exe'
    if (Test-Path $backup) { $candidates += Get-Item $backup }
    if (-not $candidates) { return $null }
    return ($candidates | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
}

function Write-CmdShim {
    param(
        [Parameter(Mandatory = $true)][string]$CmdPath,
        [Parameter(Mandatory = $true)][string]$TargetPs1
    )
    $content = @"
@echo off
REM Managed by install-local.ps1 — fork launcher (do not replace with a frozen .exe)
powershell -NoProfile -ExecutionPolicy Bypass -File "$TargetPs1" %*
"@
    Set-Content -LiteralPath $CmdPath -Value $content -Encoding ASCII
}

function Write-OfficialCmd {
    param([string]$StockExe)
    $cmdPath = Join-Path $BinDir 'grok-official.cmd'
    if (-not $StockExe -or -not (Test-Path $StockExe)) {
        $content = @"
@echo off
echo grok-official: no stock binary found under $Downloads >&2
echo Install stock Grok from https://x.ai/cli >&2
exit /b 127
"@
        Set-Content -LiteralPath $cmdPath -Value $content -Encoding ASCII
        Write-Info "warning: grok-official.cmd installed but no stock exe yet"
        return
    }
    $content = @"
@echo off
REM Always stock — escape hatch when fork launcher is broken
"$StockExe" %*
"@
    Set-Content -LiteralPath $cmdPath -Value $content -Encoding ASCII
    Write-Info "grok-official.cmd -> $StockExe"
}

function Disable-AutoUpdate {
    $cfg = Join-Path $GrokHome 'config.toml'
    if (-not (Test-Path $cfg)) {
        New-Item -ItemType Directory -Force -Path $GrokHome | Out-Null
        Set-Content -LiteralPath $cfg -Value "[cli]`nauto_update = false`n" -Encoding UTF8
        Write-Info "created $cfg with auto_update = false"
        return
    }
    $text = Get-Content -LiteralPath $cfg -Raw
    if ($text -match '(?m)^\s*auto_update\s*=') {
        $text = [regex]::Replace($text, '(?m)^\s*auto_update\s*=.*$', 'auto_update = false')
    } elseif ($text -match '(?m)^\s*\[cli\]') {
        $text = [regex]::Replace($text, '(?m)^\s*\[cli\]\s*$', "[cli]`nauto_update = false")
    } else {
        $text = $text.TrimEnd() + "`n`n[cli]`nauto_update = false`n"
    }
    Set-Content -LiteralPath $cfg -Value $text -Encoding UTF8 -NoNewline
    Write-Info "set auto_update = false in $cfg"
}

function Install-OfficialEscape {
    Ensure-Layout
    $stock = Get-NewestStockExe
    # If bin/grok.exe looks like stock and we have no downloads copy, preserve it.
    $binExe = Join-Path $BinDir 'grok.exe'
    if (-not $stock -and (Test-Path $binExe)) {
        $dest = Join-Path $BinDir 'grok.stock.exe'
        if (-not (Test-Path $dest)) {
            Copy-Item -LiteralPath $binExe -Destination $dest -Force
            Write-Info "preserved stock binary -> $dest"
        }
        $stock = $dest
    }
    Write-OfficialCmd -StockExe $stock
    $agentOfficial = Join-Path $BinDir 'agent-official.cmd'
    if ($stock -and (Test-Path $stock)) {
        Set-Content -LiteralPath $agentOfficial -Value "@echo off`r`nREM stock escape hatch`r`n`"$stock`" %*`r`n" -Encoding ASCII
    } else {
        Set-Content -LiteralPath $agentOfficial -Value "@echo off`r`necho agent-official: no stock binary`r`nexit /b 127`r`n" -Encoding ASCII
    }
}

function Seed-Build {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Die 'cargo not found — install Rust from https://rustup.rs'
    }
    Write-Info "seeding release build ($Package)…"
    Push-Location $RepoRoot
    try {
        & cargo build -p $Package --release
        if ($LASTEXITCODE -ne 0) { Die 'cargo build failed' }
    } finally {
        Pop-Location
    }
    if (-not (Test-Path $Binary)) { Die "missing $Binary after build" }
    $head = (& git -C $RepoRoot rev-parse HEAD).Trim()
    Set-Content -LiteralPath $Stamp -Value $head -NoNewline
    Write-Info "seed build ok -> $Binary"
}

function Install-Launcher {
    Ensure-Layout
    if (-not (Test-Path $LauncherPs1)) { Die "missing $LauncherPs1" }

    # PATHEXT prefers .EXE over .CMD — move stock grok.exe out of the way.
    $binExe = Join-Path $BinDir 'grok.exe'
    $agentExe = Join-Path $BinDir 'agent.exe'
    $stockBackup = Join-Path $BinDir 'grok.stock.exe'
    if (Test-Path $binExe) {
        if (-not (Test-Path $stockBackup)) {
            Move-Item -LiteralPath $binExe -Destination $stockBackup -Force
            Write-Info "moved $binExe -> $stockBackup (so grok.cmd is daily driver)"
        } else {
            Remove-Item -LiteralPath $binExe -Force
            Write-Info "removed shadowing $binExe (stock backup already at $stockBackup)"
        }
    }
    if (Test-Path $agentExe) {
        $agentBackup = Join-Path $BinDir 'agent.stock.exe'
        if (-not (Test-Path $agentBackup)) {
            Move-Item -LiteralPath $agentExe -Destination $agentBackup -Force
        } else {
            Remove-Item -LiteralPath $agentExe -Force
        }
    }

    Write-CmdShim -CmdPath (Join-Path $BinDir 'grok.cmd') -TargetPs1 $LauncherPs1
    Write-CmdShim -CmdPath (Join-Path $BinDir 'agent.cmd') -TargetPs1 $LauncherPs1
    Write-Info "wired grok.cmd + agent.cmd -> $LauncherPs1"
}

function Verify-Launcher {
    $cmd = Join-Path $BinDir 'grok.cmd'
    if (-not (Test-Path $cmd)) { Die "missing $cmd" }
    $text = Get-Content -LiteralPath $cmd -Raw
    if ($text -notmatch [regex]::Escape($LauncherPs1) -and $text -notmatch 'grok\.ps1') {
        Die "grok.cmd does not invoke scripts\grok.ps1"
    }
    if (Test-Path (Join-Path $BinDir 'grok.exe')) {
        Die "grok.exe still present in bin/ — it would shadow grok.cmd (PATHEXT). Re-run install."
    }
    Write-Info "verified: daily driver is grok.cmd -> grok.ps1"
}

function Do-Rollback {
    Ensure-Layout
    Install-OfficialEscape
    $stock = Get-NewestStockExe
    if (-not $stock) { Die "no stock binary found under $Downloads or bin\grok.stock.exe" }
    # Remove launcher cmds; restore .exe as default
    Remove-Item -LiteralPath (Join-Path $BinDir 'grok.cmd') -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath (Join-Path $BinDir 'agent.cmd') -Force -ErrorAction SilentlyContinue
    Copy-Item -LiteralPath $stock -Destination (Join-Path $BinDir 'grok.exe') -Force
    Copy-Item -LiteralPath $stock -Destination (Join-Path $BinDir 'agent.exe') -Force
    Write-Info "rolled back to stock exe: $stock"
}

function Do-Install {
    Ensure-Layout
    Install-OfficialEscape
    Install-Launcher
    if (-not $SkipBuild) {
        Seed-Build
    } else {
        Write-Info 'skipped seed build; first grok launch will build if needed'
    }
    Verify-Launcher
    if (-not $KeepAutoUpdate) {
        Disable-AutoUpdate
    }
    Write-Info ''
    Write-Info "daily driver: $(Join-Path $BinDir 'grok.cmd') -> $LauncherPs1"
    Write-Info '  (launcher: origin + upstream sync on every open; one rebuild max)'
    Write-Info 'escape hatch: grok-official'
}

if ($EnsureOfficial) {
    Install-OfficialEscape
} elseif ($Rollback) {
    Do-Rollback
} else {
    Do-Install
}
