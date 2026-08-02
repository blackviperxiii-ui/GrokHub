@echo off
setlocal EnableExtensions
REM GrokHub Windows launcher — starts Nitro UI then Electron shell.

set "APP_ROOT=%GROKHUB_HOME%"
if "%APP_ROOT%"=="" set "APP_ROOT=%~dp0.."
REM When installed under %LOCALAPPDATA%\GrokHub the .cmd lives next to desktop\
if exist "%~dp0desktop\main.mjs" set "APP_ROOT=%~dp0"
if exist "%~dp0..\desktop\main.mjs" set "APP_ROOT=%~dp0.."
for %%I in ("%APP_ROOT%") do set "APP_ROOT=%%~fI"

set "PORT=%GROKHUB_PORT%"
if "%PORT%"=="" set "PORT=18765"
set "URL=%GROKHUB_URL%"
if "%URL%"=="" set "URL=http://127.0.0.1:%PORT%"

set "RUNTIME=%LOCALAPPDATA%\GrokHub\runtime"
if not exist "%RUNTIME%" mkdir "%RUNTIME%" >nul 2>&1
set "LOG=%RUNTIME%\ui.log"
set "PIDFILE=%RUNTIME%\ui.pid"

set "GROKHUB_HOME=%APP_ROOT%"
set "GROKHUB_TRAY=%GROKHUB_TRAY%"
if "%GROKHUB_TRAY%"=="" set "GROKHUB_TRAY=1"
set "GROKHUB_URL=%URL%"

where node >nul 2>&1
if errorlevel 1 (
  echo error: node not found. Install Node.js 20+ from https://nodejs.org/
  exit /b 1
)

where electron >nul 2>&1
if errorlevel 1 (
  if exist "%APP_ROOT%\node_modules\electron\dist\electron.exe" (
    set "ELECTRON_BIN=%APP_ROOT%\node_modules\electron\dist\electron.exe"
  ) else if exist "%APP_ROOT%\node_modules\.bin\electron.cmd" (
    set "ELECTRON_BIN=%APP_ROOT%\node_modules\.bin\electron.cmd"
  ) else (
    echo error: electron not found. Run: npm install electron --save-dev
    echo   or install the full Windows package from Releases.
    exit /b 1
  )
) else (
  for /f "delims=" %%E in ('where electron') do set "ELECTRON_BIN=%%E" & goto :have_electron
)
:have_electron

if not exist "%APP_ROOT%\desktop\main.mjs" (
  echo error: GrokHub not found at %APP_ROOT%
  exit /b 1
)

REM Probe UI
curl -sf -o NUL --max-time 1 "%URL%/" >nul 2>&1
if errorlevel 1 goto start_ui
goto launch

:start_ui
if not exist "%APP_ROOT%\.output\server\index.mjs" (
  echo error: missing UI build at %APP_ROOT%\.output — run npm run desktop:build first
  exit /b 1
)
echo Starting GrokHub UI on %URL% ...
set "PORT=%PORT%"
set "NITRO_PORT=%PORT%"
set "HOST=127.0.0.1"
set "NITRO_HOST=127.0.0.1"
start "GrokHub UI" /MIN cmd /c "cd /d "%APP_ROOT%" && node .output\server\index.mjs >> "%LOG%" 2>&1"

set /a tries=0
:wait_ui
curl -sf -o NUL --max-time 1 "%URL%/" >nul 2>&1
if not errorlevel 1 goto launch
set /a tries+=1
if %tries% GEQ 80 (
  echo error: UI failed to start — see %LOG%
  exit /b 1
)
timeout /t 0 /nobreak >nul
ping -n 1 127.0.0.1 >nul
goto wait_ui

:launch
set "GROKHUB_URL=%URL%"
REM AppUserModelID for taskbar pin / jump lists
set "ELECTRON_FORCE_WINDOW_MENU_BAR=0"
"%ELECTRON_BIN%" "%APP_ROOT%\desktop\main.mjs" %*
endlocal
