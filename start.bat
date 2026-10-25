@echo off
cd /d "%~dp0"
echo Starting Kindle Paperwhite Desktop Monitor (Port 28080)...
start "" "target\release\paperwhite-desktop-monitor.exe"
echo Server started in background.
echo Access URL: Check logs\server.log for your PC's LAN IP (e.g. http://192.168.x.x:28080/)
