@echo off
echo ========================================================
echo   Remove Kindle Desktop Monitor from Windows Startup
echo ========================================================
echo.
powershell -NoProfile -ExecutionPolicy Bypass -Command "$startup = [Environment]::GetFolderPath('Startup'); $lnk = Join-Path $startup 'KindleDesktopMonitor.lnk'; if (Test-Path $lnk) { Remove-Item $lnk -Force; Write-Host '[OK] Startup shortcut removed successfully.' } else { Write-Host '[INFO] No startup shortcut found.' }"
echo.
pause
