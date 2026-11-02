"""فحص تطابق الملفات المُولَّدة مع الملف الموحّد contracts.toml."""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GENERATOR = ROOT / "quality" / "scripts" / "generate.py"


def _run(flag: str) -> int:
    result = subprocess.run(
        [sys.executable, str(GENERATOR), *([flag] if flag else [])],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.stdout.strip():
        print(result.stdout.rstrip())
    if result.returncode != 0 and result.stderr.strip():
        print(result.stderr.rstrip())
    return result.returncode


def generate() -> int:
    return _run("")


def run() -> int:
    return _run("--check")
