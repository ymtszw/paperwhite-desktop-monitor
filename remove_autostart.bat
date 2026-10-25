@echo off
echo ========================================================
echo   Remove Kindle Desktop Monitor from Windows Startup
echo ========================================================
echo.
powershell -NoProfile -ExecutionPolicy Bypass -Command "Remove-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'KindleDesktopMonitor' -ErrorAction SilentlyContinue; $oldLnk = Join-Path ([Environment]::GetFolderPath('Startup')) 'KindleDesktopMonitor.lnk'; if (Test-Path $oldLnk) { Remove-Item $oldLnk -Force }; Write-Host '[OK] Startup entry removed from Windows Registry.'"
echo.
pause
