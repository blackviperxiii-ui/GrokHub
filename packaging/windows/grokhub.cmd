@echo off
setlocal
REM Portable / side-by-side launcher when GrokHub.exe is not used.
set "HERE=%~dp0"
if defined GROKHUB_HOME goto :run
set "GROKHUB_HOME=%HERE%"
if exist "%HERE%..\desktop\main.mjs" set "GROKHUB_HOME=%HERE%.."
:run
set "GROKHUB_EXEC=%~f0"
set ELECTRON_NO_ATTACH_CONSOLE=1
if exist "%GROKHUB_HOME%\GrokHub.exe" (
  start "" "%GROKHUB_HOME%\GrokHub.exe" %*
  exit /b 0
)
if exist "%GROKHUB_HOME%\electron-runtime\electron.exe" (
  "%GROKHUB_HOME%\electron-runtime\electron.exe" "%GROKHUB_HOME%\desktop\main.mjs" %*
  exit /b %ERRORLEVEL%
)
where electron >nul 2>&1 && (
  electron "%GROKHUB_HOME%\desktop\main.mjs" %*
  exit /b %ERRORLEVEL%
)
echo GrokHub: no Electron runtime found.
exit /b 1
