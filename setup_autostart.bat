@echo off
cd /d "%~dp0"
echo ========================================================
echo   Register Kindle Desktop Monitor to Windows Startup
echo ========================================================
echo.
powershell -NoProfile -ExecutionPolicy Bypass -Command "$exe = (Resolve-Path 'target\release\paperwhite-desktop-monitor.exe').Path; Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'KindleDesktopMonitor' -Value $exe; $oldLnk = Join-Path ([Environment]::GetFolderPath('Startup')) 'KindleDesktopMonitor.lnk'; if (Test-Path $oldLnk) { Remove-Item $oldLnk -Force }; Write-Host '[OK] Registered to HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'; Write-Host 'Value:' $exe"
if %ERRORLEVEL% equ 0 (
    echo.
    echo [OK] Successfully registered!
    echo The monitor server will automatically start in the background when Windows boots.
) else (
    echo [ERROR] Failed to create startup shortcut.
)
echo.
pause
