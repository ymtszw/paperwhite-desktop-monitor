@echo off
cd /d "%~dp0"
echo ========================================================
echo   Register Kindle Desktop Monitor to Windows Startup
echo ========================================================
echo.
powershell -NoProfile -ExecutionPolicy Bypass -Command "$ws = New-Object -ComObject WScript.Shell; $startup = [Environment]::GetFolderPath('Startup'); $s = $ws.CreateShortcut((Join-Path $startup 'KindleDesktopMonitor.lnk')); $s.TargetPath = 'wscript.exe'; $vbs = Join-Path (Get-Location) 'start_hidden.vbs'; $s.Arguments = ('\"' + $vbs + '\"'); $s.WorkingDirectory = (Get-Location).Path; $s.Description = 'Kindle Desktop Monitor'; $s.Save(); Write-Host 'Shortcut created in:' $startup"
if %ERRORLEVEL% equ 0 (
    echo.
    echo [OK] Successfully registered!
    echo The monitor server will automatically start in the background when Windows boots.
) else (
    echo [ERROR] Failed to create startup shortcut.
)
echo.
pause
