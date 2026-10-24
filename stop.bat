@echo off
echo Stopping Kindle Paperwhite Desktop Monitor...
taskkill /F /IM paperwhite-desktop-monitor.exe >nul 2>&1
if %ERRORLEVEL% equ 0 (
    echo Successfully stopped.
) else (
    echo Process was not running.
)
pause
