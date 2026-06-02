@echo off
:: install_startup.bat
:: Adds EarbudsTracker to Windows startup via Task Scheduler
:: (runs without a console window at login)

set "SCRIPT_DIR=%~dp0"
set "PYTHON_EXE=C:\Users\HP\AppData\Local\Programs\Python\Python313\pythonw.exe"
set "MAIN_SCRIPT=%SCRIPT_DIR%main.py"
set "TASK_NAME=EarbudsTracker"

echo Installing EarbudsTracker startup task...

schtasks /create /tn "%TASK_NAME%" ^
  /tr "\"%PYTHON_EXE%\" \"%MAIN_SCRIPT%\"" ^
  /sc ONLOGON ^
  /rl HIGHEST ^
  /f

if %errorlevel% equ 0 (
    echo.
    echo [OK] Task Scheduler entry created: %TASK_NAME%
    echo      EarbudsTracker will start automatically on next login.
) else (
    echo.
    echo [ERROR] Failed to create task. Try running this script as Administrator.
)
pause
