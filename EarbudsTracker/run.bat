@echo off
:: run.bat – Launch EarbudsTracker with the correct Python interpreter
:: Uses pythonw.exe (no console window) from Python313 install
set "PY=C:\Users\HP\AppData\Local\Programs\Python\Python313\pythonw.exe"
set "SCRIPT=%~dp0main.py"

echo Starting EarbudsTracker...
start "" "%PY%" "%SCRIPT%"
echo Done. Check system tray.
timeout /t 2 >nul
