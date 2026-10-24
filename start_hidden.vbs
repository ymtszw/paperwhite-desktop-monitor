Set fso = CreateObject("Scripting.FileSystemObject")
currentDir = fso.GetParentFolderName(WScript.ScriptFullName)
exePath = fso.BuildPath(currentDir, "target\release\paperwhite-desktop-monitor.exe")

Set WshShell = CreateObject("WScript.Shell")
WshShell.CurrentDirectory = currentDir
WshShell.Run Chr(34) & exePath & Chr(34), 0, False
