#Requires -Version 5.1
<#
.SYNOPSIS
  Grok Build fork launcher for Windows.

.DESCRIPTION
  On every start (unless skipped):
    1. git fetch upstream
    2. if upstream/main moved, rebase our fork branch onto it
    3. rebuild release binary only when HEAD changed (or binary missing)
    4. install into ~/.grok/bin (replacing stock) when the binary changes
    5. exec the fork binary with all args

  Skip sync/rebuild:  $env:GROK_SKIP_SYNC = '1'
  Skip rebuild only:  $env:GROK_SKIP_REBUILD = '1'  (still fetches/reports)
  Skip install step:  $env:GROK_SKIP_INSTALL = '1'
  Stock solid bg:     $env:GROK_SOLID_BG = '1'
#>

$ErrorActionPreference = 'Stop'

$Repo = if ($env:GROK_FORK_REPO) { $env:GROK_FORK_REPO } else { Join-Path $HOME 'projects\grok-build' }
$Branch = if ($env:GROK_FORK_BRANCH) { $env:GROK_FORK_BRANCH } else { 'fork/transparent-bg' }
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

function Acquire-Lock {
    $waited = 0
    while ($true) {
        try {
            New-Item -ItemType Directory -Path $LockDir -ErrorAction Stop | Out-Null
            break
        } catch {
            if ($waited -ge 600) {
                Die "timed out waiting for launch lock ($LockDir). Remove it if stuck."
            }
            Start-Sleep -Seconds 1
            $waited++
        }
    }
}

function Release-Lock {
    if (Test-Path $LockDir) {
        Remove-Item -LiteralPath $LockDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Test-NeedRebuild {
    if (-not (Test-Path $Bin)) { return $true }
    if (-not (Test-Path $Stamp)) { return $true }
    $head = (& git -C $Repo rev-parse HEAD).Trim()
    $built = (Get-Content -LiteralPath $Stamp -Raw -ErrorAction SilentlyContinue).Trim()
    return ($head -ne $built)
}

function Sync-Upstream {
    if (-not (Test-Path (Join-Path $Repo '.git'))) {
        Die "repo not found: $Repo"
    }
    Push-Location $Repo
    try {
        $remotes = & git remote
        if ($remotes -notcontains $UpstreamRemote) {
            Die "missing remote '$UpstreamRemote' (expected xai-org/grok-build)"
        }

        $current = (& git rev-parse --abbrev-ref HEAD).Trim()
        if ($current -ne $Branch) {
            Write-ForkLog "checking out $Branch (was on $current)"
            & git checkout $Branch
            if ($LASTEXITCODE -ne 0) { Die "could not checkout $Branch" }
        }

        $stashed = $false
        $status = & git status --porcelain
        if ($status) {
            Write-ForkLog 'stashing local uncommitted changes for rebase'
            $stampMsg = "grok-fork-launch $([DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mmZ'))"
            & git stash push -u -m $stampMsg | Out-Null
            $stashed = $true
        }

        Write-ForkLog "fetching $UpstreamRemote/$UpstreamRef ..."
        & git fetch --quiet $UpstreamRemote $UpstreamRef
        if ($LASTEXITCODE -ne 0) {
            Write-ForkLog 'warning: fetch failed (offline?). continuing with current tree.'
            if ($stashed) {
                & git stash pop 2>$null | Out-Null
                if ($LASTEXITCODE -ne 0) { Write-ForkLog 'warning: could not restore stash' }
            }
            return
        }

        $head = (& git rev-parse HEAD).Trim()
        $up = (& git rev-parse "$UpstreamRemote/$UpstreamRef").Trim()
        $base = (& git merge-base HEAD "$UpstreamRemote/$UpstreamRef").Trim()

        if ($base -ne $up) {
            $behind = (& git rev-list --count "HEAD..$UpstreamRemote/$UpstreamRef" 2>$null)
            if (-not $behind) { $behind = '?' }
            Write-ForkLog "upstream is $behind commit(s) ahead - rebasing $Branch onto $UpstreamRemote/$UpstreamRef"
            & git rebase "$UpstreamRemote/$UpstreamRef"
            if ($LASTEXITCODE -ne 0) {
                Write-ForkLog "rebase hit conflicts. Resolve in $Repo, then:"
                Write-ForkLog '  git rebase --continue'
                Write-ForkLog '  cargo build -p xai-grok-pager-bin --release'
                Write-ForkLog "  git rev-parse HEAD > $Stamp"
                if ($stashed) { Write-ForkLog "(your stash is still in 'git stash list')" }
                exit 1
            }
            $short = (& git rev-parse --short HEAD).Trim()
            Write-ForkLog "rebase complete ($short)"
        } else {
            $short = (& git rev-parse --short HEAD).Trim()
            Write-ForkLog "already up to date with $UpstreamRemote/$UpstreamRef ($short)"
        }

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
        Write-ForkLog 'warning: PROTOC not set; ensure protoc is available (see FORK.md Windows notes)'
    }

    Write-ForkLog 'building release binary (this can take a few minutes) ...'
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
            # Common when an older grok is still running — fall back to running from target/
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
        Acquire-Lock
        try {
            if (-not $skipSync) { Sync-Upstream }
            if (-not $skipRebuild) { Rebuild-IfNeeded }
        } finally {
            Release-Lock
        }
    }

    if (-not (Test-Path $Bin)) {
        Die "binary not found after build: $Bin"
    }

    if (-not $skipInstall) {
        [void](Install-OverStock)
    }

    # Prefer installed copy when present and matches; always can run from target.
    $run = $Bin
    $installed = Join-Path $InstallDir 'grok.exe'
    if ((Test-Path $installed) -and -not $skipInstall) {
        $run = $installed
    }

    Write-ForkLog "exec $($run)"
    # Replace this process with the binary so Ctrl+C / exit codes behave normally.
    & $run @args
    exit $LASTEXITCODE
}

Main @args
