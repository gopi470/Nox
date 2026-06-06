from pathlib import Path
import subprocess
import sys

script = Path(__file__).resolve().parents[2] / "regen_icons.ps1"

if not script.exists():
    raise FileNotFoundError(f"Missing icon generator: {script}")

subprocess.run(
    [
        "powershell",
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        str(script),
    ],
    check=True,
)

