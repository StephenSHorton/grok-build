#Requires -Version 5.1
<#
.SYNOPSIS
  Grok Build fork launcher for Windows.

.DESCRIPTION
  On every start (unless skipped) - one pass, one rebuild max:
    1. git fetch origin (your GitHub fork)
    2. git fetch upstream (xai-org/grok-build)
    3. fast-forward onto origin when behind; rebase onto upstream when moved
    4. rebuild release binary only when HEAD changed (or binary missing)
    5. install into ~/.grok/bin (replacing stock) when the binary changes
    6. exec the fork binary with all args

  Skip sync/rebuild:  $env:GROK_SKIP_SYNC = '1'
  Skip rebuild only:  $env:GROK_SKIP_REBUILD = '1'  (still fetches/reports)
  Skip install step:  $env:GROK_SKIP_INSTALL = '1'
  Stock solid bg:     $env:GROK_SOLID_BG = '1'
#>

$ErrorActionPreference = 'Stop'

# Default repo = parent of this script (works for any clone path).
if ($env:GROK_FORK_REPO) {
    $Repo = $env:GROK_FORK_REPO
} else {
    $Repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
}
$Branch = if ($env:GROK_FORK_BRANCH) { $env:GROK_FORK_BRANCH } else { 'main' }
$OriginRemote = if ($env:GROK_ORIGIN_REMOTE) { $env:GROK_ORIGIN_REMOTE } else { 'origin' }
$OriginRef = if ($env:GROK_ORIGIN_REF) { $env:GROK_ORIGIN_REF } else { $Branch }
$UpstreamRemote = if ($env:GROK_UPSTREAM_REMOTE) { $env:GROK_UPSTREAM_REMOTE } else { 'upstream' }
$UpstreamRef = if ($env:GROK_UPSTREAM_REF) { $env:GROK_UPSTREAM_REF } else { 'main' }
$Bin = Join-Path $Repo 'target\release\xai-grok-pager.exe'
$Stamp = Join-Path $Repo '.fork-built-at'
$LockDir = Join-Path $Repo '.fork-launch.lock'
$InstallDir = if ($env:GROK_INSTALL_DIR) { $env:GROK_INSTALL_DIR } else { Join-Path $HOME '.grok\bin' }
$ProtocDir = Join-Path $Repo '.tools\protoc\bin'

# Prefer cargo tools + local protoc for rebuilds
$env:Path = @(
    (Join-Path $HOME '.cargo\bin'),
    $ProtocDir,
    $env:Path
) -join ';'

if (Test-Path (Join-Path $ProtocDir 'protoc.exe')) {
    $env:PROTOC = Join-Path $ProtocDir 'protoc.exe'
}

function Write-ForkLog([string]$Message) {
    [Console]::Error.WriteLine("[grok-fork] $Message")
}

function Die([string]$Message) {
    Write-ForkLog "error: $Message"
    exit 1
}

function Test-LockHolderAlive {
    $pidFile = Join-Path $LockDir 'pid'
    if (-not (Test-Path -LiteralPath $pidFile)) { return $false }
    $raw = Get-Content -LiteralPath $pidFile -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $raw) { return $false }
    $holderPid = 0
    if (-not [int]::TryParse($raw.Trim(), [ref]$holderPid)) { return $false }
    if ($holderPid -le 0) { return $false }
    return [bool](Get-Process -Id $holderPid -ErrorAction SilentlyContinue)
}

function Clear-Lock {
    if (Test-Path -LiteralPath $LockDir) {
        Remove-Item -LiteralPath $LockDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# Short lock wait - never block launch for minutes. Recover stale locks left
# behind when a previous launcher was killed so we do not sit silent.
# Returns $true if this process owns the lock.
function Acquire-Lock {
    $waited = 0
    while ($true) {
        try {
            New-Item -ItemType Directory -Path $LockDir -ErrorAction Stop | Out-Null
            Set-Content -LiteralPath (Join-Path $LockDir 'pid') -Value $PID
            return $true
        } catch {
            if (-not (Test-LockHolderAlive)) {
                Write-ForkLog 'removing stale launch lock (no live holder)'
                Clear-Lock
                continue
            }
            $pidFile = Join-Path $LockDir 'pid'
            $holder = '?'
            if (Test-Path -LiteralPath $pidFile) {
                $holder = (Get-Content -LiteralPath $pidFile -ErrorAction SilentlyContinue | Select-Object -First 1)
                if (-not $holder) { $holder = '?' }
            }
            if ($waited -eq 0) {
                Write-ForkLog "waiting for another launch (pid $holder) to finish sync ..."
            } elseif (($waited % 5) -eq 0) {
                Write-ForkLog "still waiting for launch lock (${waited}s) ..."
            }
            if ($waited -ge 15) {
                Write-ForkLog 'another launch is still syncing - skipping sync this time'
                return $false
            }
            Start-Sleep -Seconds 1
            $waited++
        }
    }
}

function Release-Lock {
    Clear-Lock
}

function Test-NeedRebuild {
    if (-not (Test-Path $Bin)) { return $true }
    if (-not (Test-Path $Stamp)) { return $true }
    $head = (& git -C $Repo rev-parse HEAD).Trim()
    $built = (Get-Content -LiteralPath $Stamp -Raw -ErrorAction SilentlyContinue).Trim()
    return ($head -ne $built)
}

function Integrate-Origin {
    $remotes = & git remote
    if ($remotes -notcontains $OriginRemote) {
        Write-ForkLog "no remote '$OriginRemote' - skipping fork pull"
        return
    }
    $originTip = & git rev-parse "$OriginRemote/$OriginRef" 2>$null
    if ($LASTEXITCODE -ne 0 -or -not $originTip) {
        Write-ForkLog "warning: missing $OriginRemote/$OriginRef after fetch - skipping fork pull"
        return
    }
    $originTip = $originTip.Trim()
    $head = (& git rev-parse HEAD).Trim()
    if ($head -eq $originTip) {
        $short = (& git rev-parse --short "$OriginRemote/$OriginRef").Trim()
        Write-ForkLog "already up to date with $OriginRemote/$OriginRef ($short)"
        return
    }
    $base = (& git merge-base HEAD "$OriginRemote/$OriginRef").Trim()
    if ($base -eq $head) {
        $behind = (& git rev-list --count "HEAD..$OriginRemote/$OriginRef" 2>$null)
        if (-not $behind) { $behind = '?' }
        Write-ForkLog "origin is $behind commit(s) ahead - fast-forwarding $Branch to $OriginRemote/$OriginRef"
        & git merge --ff-only "$OriginRemote/$OriginRef" 2>$null | Out-Null
        if ($LASTEXITCODE -eq 0) {
            $short = (& git rev-parse --short HEAD).Trim()
            Write-ForkLog "fast-forward complete ($short)"
        } else {
            Write-ForkLog 'warning: fast-forward from origin failed - launching current tree'
        }
        return
    }
    if ($base -eq $originTip) {
        $ahead = (& git rev-list --count "$OriginRemote/$OriginRef..HEAD" 2>$null)
        if (-not $ahead) { $ahead = '?' }
        Write-ForkLog "local is $ahead commit(s) ahead of $OriginRemote/$OriginRef (not pushed; ok)"
        return
    }
    $behind = (& git rev-list --count "HEAD..$OriginRemote/$OriginRef" 2>$null)
    if (-not $behind) { $behind = '?' }
    Write-ForkLog "origin and local have diverged (+$behind on origin) - not auto-merging"
}

function Integrate-Upstream {
    $remotes = & git remote
    if ($remotes -notcontains $UpstreamRemote) {
        Write-ForkLog "warning: missing remote '$UpstreamRemote' - skipping upstream rebase"
        return
    }
    $up = & git rev-parse "$UpstreamRemote/$UpstreamRef" 2>$null
    if ($LASTEXITCODE -ne 0 -or -not $up) {
        Write-ForkLog "warning: missing $UpstreamRemote/$UpstreamRef after fetch - skipping upstream rebase"
        return
    }
    $up = $up.Trim()
    $base = (& git merge-base HEAD "$UpstreamRemote/$UpstreamRef").Trim()
    if ($base -eq $up) {
        $short = (& git rev-parse --short HEAD).Trim()
        Write-ForkLog "already up to date with $UpstreamRemote/$UpstreamRef ($short)"
        return
    }
    $behind = (& git rev-list --count "HEAD..$UpstreamRemote/$UpstreamRef" 2>$null)
    if (-not $behind) { $behind = '?' }
    Write-ForkLog "upstream is $behind commit(s) ahead - rebasing $Branch onto $UpstreamRemote/$UpstreamRef"
    & git rebase "$UpstreamRemote/$UpstreamRef"
    if ($LASTEXITCODE -ne 0) {
        Write-ForkLog "rebase hit conflicts - aborting and continuing with current tree"
        & git rebase --abort 2>$null | Out-Null
        return
    }
    $short = (& git rev-parse --short HEAD).Trim()
    Write-ForkLog "rebase complete ($short)"
}

function Sync-Remotes {
    if (-not (Test-Path (Join-Path $Repo '.git'))) {
        Die "repo not found: $Repo"
    }
    Push-Location $Repo
    try {
        $current = (& git rev-parse --abbrev-ref HEAD).Trim()
        if ($current -ne $Branch) {
            Write-ForkLog "checking out $Branch (was on $current)"
            & git checkout $Branch
            if ($LASTEXITCODE -ne 0) { Die "could not checkout $Branch" }
        }

        $stashed = $false
        # Tracked-only (never -u - untracked can include huge trees)
        $status = & git status --porcelain -uno
        if ($status) {
            Write-ForkLog 'stashing tracked local changes for sync (untracked left alone)'
            $stampMsg = "grok-fork-launch $([DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mmZ'))"
            & git stash push -m $stampMsg | Out-Null
            $stashed = $true
        }

        Write-ForkLog "fetching $OriginRemote + $UpstreamRemote ..."
        $remotes = & git remote
        if ($remotes -contains $OriginRemote) {
            & git fetch --quiet $OriginRemote $OriginRef 2>$null
            if ($LASTEXITCODE -ne 0) { Write-ForkLog "warning: fetch $OriginRemote failed (offline?)" }
        }
        if ($remotes -contains $UpstreamRemote) {
            & git fetch --quiet $UpstreamRemote $UpstreamRef 2>$null
            if ($LASTEXITCODE -ne 0) { Write-ForkLog "warning: fetch $UpstreamRemote failed (offline?)" }
        } else {
            Write-ForkLog "warning: missing remote '$UpstreamRemote'"
        }

        Integrate-Origin
        Integrate-Upstream

        if ($stashed) {
            & git stash pop 2>$null | Out-Null
            if ($LASTEXITCODE -eq 0) {
                Write-ForkLog 'restored stashed changes'
            } else {
                Write-ForkLog "warning: stash pop had conflicts - check 'git stash list' / status"
            }
        }
    } finally {
        Pop-Location
    }
}

function Rebuild-IfNeeded {
    if (-not (Test-NeedRebuild)) {
        Write-ForkLog 'binary current'
        return
    }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Die 'cargo not on PATH; install Rust or set PATH to include ~/.cargo/bin'
    }
    if (-not (Get-Command dotslash -ErrorAction SilentlyContinue)) {
        Write-ForkLog 'warning: dotslash not found; build may need protoc on PATH / PROTOC'
    }
    if (-not $env:PROTOC) {
        Write-ForkLog 'warning: PROTOC not set; ensure protoc is available (see AGENTS.md)'
    }

    Write-ForkLog 'building release binary once (HEAD moved or binary missing) ...'
    Push-Location $Repo
    try {
        & cargo build -p xai-grok-pager-bin --release
        if ($LASTEXITCODE -ne 0) { Die 'cargo build failed' }
        $head = (& git rev-parse HEAD).Trim()
        Set-Content -LiteralPath $Stamp -Value $head -NoNewline
        Write-ForkLog "build ok -> $Bin"
    } finally {
        Pop-Location
    }
}

function Install-OverStock {
    if (-not (Test-Path $Bin)) {
        Die "binary not found: $Bin"
    }
    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    }

    $targets = @(
        (Join-Path $InstallDir 'grok.exe'),
        (Join-Path $InstallDir 'agent.exe')
    )

    foreach ($dest in $targets) {
        $name = Split-Path $dest -Leaf
        $backup = Join-Path $InstallDir ($name -replace '\.exe$', '.upstream.exe')
        if ((Test-Path $dest) -and -not (Test-Path $backup)) {
            # Keep one stock backup so we can restore if needed
            try {
                Copy-Item -LiteralPath $dest -Destination $backup -Force -ErrorAction Stop
                Write-ForkLog "backed up stock $name -> $backup"
            } catch {
                Write-ForkLog "warning: could not back up $name (in use?): $_"
            }
        }

        try {
            Copy-Item -LiteralPath $Bin -Destination $dest -Force -ErrorAction Stop
        } catch {
            # Common when an older grok is still running - fall back to running from target/
            Write-ForkLog "warning: could not install $name (file in use?). Will run from repo target."
            return $false
        }
    }
    Write-ForkLog "installed fork binary into $InstallDir"
    return $true
}

function Main {
    if (-not (Test-Path (Join-Path $Repo '.git'))) {
        Die "not a git repo: $Repo"
    }

    $skipSync = $env:GROK_SKIP_SYNC -eq '1'
    $skipRebuild = $env:GROK_SKIP_REBUILD -eq '1'
    $skipInstall = $env:GROK_SKIP_INSTALL -eq '1'

    if (-not $skipSync -or -not $skipRebuild) {
        if (Acquire-Lock) {
            try {
                if (-not $skipSync) { Sync-Remotes }
                if (-not $skipRebuild) { Rebuild-IfNeeded }
            } finally {
                Release-Lock
            }
        }
    }

    if (-not (Test-Path $Bin)) {
        Die "binary not found after build: $Bin"
    }

    # Daily driver is grok.cmd -> this script. Always exec the in-repo release
    # binary (or GROK_FORK_BIN). Do not copy over bin\grok.exe - that shadows
    # the launcher (PATHEXT prefers .exe over .cmd).
    if (-not $skipInstall -and $env:GROK_INSTALL_OVER_STOCK -eq '1') {
        Write-ForkLog 'GROK_INSTALL_OVER_STOCK=1 - copying into .grok\bin (not recommended)'
        [void](Install-OverStock)
    }

    $run = $Bin
    Write-ForkLog "exec $run"
    & $run @args
    exit $LASTEXITCODE
}

Main @args
