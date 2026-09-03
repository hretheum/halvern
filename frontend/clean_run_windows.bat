@echo off
setlocal

REM Development run on Windows. See clean_build_windows.bat for why this uses
REM `tauri:dev` rather than `tauri dev`.
REM
REM Pass a log level as the first argument: clean_run_windows.bat debug

if "%~1"=="" (set RUST_LOG=info) else (set RUST_LOG=%~1)
echo Log level: %RUST_LOG%

echo Cleaning previous builds...
if exist .next rd /s /q .next
if exist out rd /s /q out

echo Cleaning dependencies...
if exist node_modules rd /s /q node_modules

echo Installing dependencies...
call pnpm install
if errorlevel 1 exit /b %errorlevel%

echo Starting the app...
call pnpm run tauri:dev
if errorlevel 1 exit /b %errorlevel%

endlocal
