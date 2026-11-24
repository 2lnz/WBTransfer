@echo off
setlocal
cd /d "%~dp0"

where cargo >nul 2>nul
if errorlevel 1 (
    echo [ERROR] cargo not found.
    echo Install Rust first: https://rustup.rs
    pause
    exit /b 1
)

cargo build --release
if errorlevel 1 (
    echo.
    echo BUILD FAILED
    pause
    exit /b 1
)

rem Optional: embed app.ico into the exe resources so Explorer shows the icon too.
rem Requires python. Skipped if python or app.ico is missing (window icon still loads at runtime).
where python >nul 2>nul
if errorlevel 1 goto skip_icon
if not exist app.ico goto skip_icon
if exist "target\release\wbtransfer.exe" python embed_icon.py app.ico "target\release\wbtransfer.exe"
if exist "target\x86_64-pc-windows-gnu\release\wbtransfer.exe" python embed_icon.py app.ico "target\x86_64-pc-windows-gnu\release\wbtransfer.exe"
:skip_icon

echo.
echo Build OK.
echo   MSVC toolchain: target\release\wbtransfer.exe
echo   GNU  toolchain: target\x86_64-pc-windows-gnu\release\wbtransfer.exe
echo.
echo Keep app.ico next to the exe for the window / taskbar icon.
pause
