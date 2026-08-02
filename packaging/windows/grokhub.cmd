@echo off
setlocal EnableExtensions
REM GrokHub Windows launcher
REM Layout: %LOCALAPPDATA%\GrokHub\  with desktop\, .output\, node_modules\, grokhub.cmd

set "SCRIPT_DIR=%~dp0"
set "SCRIPT_DIR=%SCRIPT_DIR:~0,-1%"

set "APP_ROOT="
if defined GROKHUB_HOME if exist "%GROKHUB_HOME%\desktop\main.mjs" set "APP_ROOT=%GROKHUB_HOME%"
if not defined APP_ROOT if exist "%SCRIPT_DIR%\desktop\main.mjs" set "APP_ROOT=%SCRIPT_DIR%"
if not defined APP_ROOT if exist "%SCRIPT_DIR%\..\desktop\main.mjs" for %%I in ("%SCRIPT_DIR%\..") do set "APP_ROOT=%%~fI"

if not defined APP_ROOT (
  echo error: GrokHub not found. Expected desktop\main.mjs beside this script.
  echo        Run: powershell -ExecutionPolicy Bypass -File packaging\windows\install.ps1 -Build
  exit /b 1
)

set "GROKHUB_HOME=%APP_ROOT%"
if not defined GROKHUB_PORT set "GROKHUB_PORT=18765"
if not defined GROKHUB_URL set "GROKHUB_URL=http://127.0.0.1:%GROKHUB_PORT%"
if not defined GROKHUB_TRAY set "GROKHUB_TRAY=1"

where node >nul 2>&1
if errorlevel 1 (
  echo error: Node.js not found. Install LTS from https://nodejs.org/
  exit /b 1
)

set "ELECTRON_BIN="
if exist "%APP_ROOT%\node_modules\electron\dist\electron.exe" set "ELECTRON_BIN=%APP_ROOT%\node_modules\electron\dist\electron.exe"
if not defined ELECTRON_BIN if exist "%APP_ROOT%\node_modules\.bin\electron.cmd" set "ELECTRON_BIN=%APP_ROOT%\node_modules\.bin\electron.cmd"
if not defined ELECTRON_BIN (
  where electron >nul 2>&1
  if not errorlevel 1 for /f "delims=" %%E in ('where electron') do if not defined ELECTRON_BIN set "ELECTRON_BIN=%%E"
)

if not defined ELECTRON_BIN (
  echo error: Electron not found at %APP_ROOT%\node_modules\electron\dist\electron.exe
  echo        Re-run install.ps1 -Build
  exit /b 1
)

if not exist "%APP_ROOT%\desktop\main.mjs" (
  echo error: missing %APP_ROOT%\desktop\main.mjs
  exit /b 1
)

REM Electron main (ui-server.cjs) starts Nitro if needed.
cd /d "%APP_ROOT%"
"%ELECTRON_BIN%" "%APP_ROOT%\desktop\main.mjs" %*
exit /b %ERRORLEVEL%
