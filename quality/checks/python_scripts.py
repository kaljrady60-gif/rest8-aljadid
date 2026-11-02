"""فحص سكربتات Python بأداة Ruff (سريعة، مكتوبة بـ Rust، مجانية)."""
from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def run() -> int:
    if not (shutil.which("ruff") or shutil.which("ruff.exe")):
        # التثبيت: pip install ruff — أو تشغيل الوحدة مباشرة.
        cmd = ["python3", "-m", "ruff", "check", "quality/"]
    else:
        cmd = ["ruff", "check", "quality/"]
    result = subprocess.run(cmd, cwd=ROOT, check=False)
    return result.returncode
