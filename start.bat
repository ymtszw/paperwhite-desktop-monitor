@echo off
cd /d "%~dp0"
echo Starting Kindle Paperwhite Desktop Monitor (Port 28080)...
start "" "target\release\paperwhite-desktop-monitor.exe"
echo Server started.
echo Access URL: http://192.168.0.2:28080/
