@echo off
cd /d "%~dp0"
echo Starting Kindle Paperwhite Desktop Monitor...
start "" "target\release\paperwhite-desktop-monitor.exe"
echo Server started in background.
echo Check Kindle at http://192.168.0.2:8080/
