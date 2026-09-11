@echo off
REM grok-fork: fetch origin/main + upstream/main, rebuild if rust moved, exec.
REM Does not overwrite official grok. Repo copy: ROOT is parent of scripts/.
setlocal
set "GROK_PROCESS_BRAND=fork"
if not defined GROK_DISABLE_AUTOUPDATER set "GROK_DISABLE_AUTOUPDATER=1"
if not defined GROK_MCP_CHANNELS set "GROK_MCP_CHANNELS=1"
if defined GROK_FORK_REPO (
  set "ROOT=%GROK_FORK_REPO%"
) else (
  set "ROOT=%~dp0.."
)
title Grok-fork
powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%\scripts\grok-fork.ps1" %*
