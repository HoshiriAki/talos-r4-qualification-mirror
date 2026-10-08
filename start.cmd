@echo off
setlocal EnableExtensions DisableDelayedExpansion
chcp 65001 >nul 2>&1

cd /d "%~dp0" || (
  echo [TALOS] Failed to enter repository directory: %~dp0
  exit /b 70
)

title TALOS Runtime Supervisor

if not exist "scripts\start-supervisor.mjs" (
  echo [TALOS] Missing scripts\start-supervisor.mjs
  echo [TALOS] Pull the latest prototype branch and retry.
  exit /b 66
)

where node.exe >nul 2>&1
if errorlevel 1 (
  echo [TALOS] Node.js was not found in PATH.
  echo [TALOS] Install Node.js 22, reopen the terminal, then run start.cmd doctor.
  exit /b 69
)

node "scripts\start-supervisor.mjs" %*
set "TALOS_EXIT=%ERRORLEVEL%"

if not "%TALOS_EXIT%"=="0" (
  echo.
  echo [TALOS] Runtime supervisor exited with code %TALOS_EXIT%.
  echo [TALOS] Review .talos-runtime\latest.txt and the session diagnostics directory.
  if /I "%TALOS_PAUSE_ON_ERROR%"=="1" pause
)

endlocal & exit /b %TALOS_EXIT%
