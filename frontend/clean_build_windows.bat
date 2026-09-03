@echo off
setlocal

REM Production build on Windows.
REM
REM Goes through `tauri:build`, not `tauri build`. The npm script runs
REM scripts/tauri-auto.js, which builds the llama-helper sidecar into
REM src-tauri\binaries\llama-helper-<triple>.exe first. Tauri's externalBin
REM aborts the build when that file is missing, so calling `tauri build`
REM directly fails on any checkout that has not built the sidecar by hand.
REM
REM Requires: Visual Studio Build Tools with the "Desktop development with C++"
REM workload, CMake, and Node >= 22.13. See docs\BUILDING.md.

if "%RUST_LOG%"=="" set RUST_LOG=info

echo Cleaning previous builds...
if exist .next rd /s /q .next
if exist out rd /s /q out

echo Cleaning dependencies...
if exist node_modules rd /s /q node_modules

echo Installing dependencies...
call pnpm install
if errorlevel 1 exit /b %errorlevel%

echo Building the project...
call pnpm run tauri:build
if errorlevel 1 exit /b %errorlevel%

endlocal
