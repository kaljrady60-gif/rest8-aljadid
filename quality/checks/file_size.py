"""حارس حجم الملفات: يمنع تضخّم أي ملف فوق السقف المعلن في contracts.toml.

الهدف: صيانة سهلة. الملف الصغير يُقرأ ويُراجع ويُحدَّث دون مخاطرة.
السقوف تُقرأ من الملف الموحّد لا من ثوابت مكرّرة هنا.
"""
from __future__ import annotations

import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CONTRACTS = ROOT / "quality" / "contracts.toml"

# الملفات المولّدة مستثناة: لا تُحرَّر يدويًا فحدّ الحجم لا معنى له فيها.
SKIP_DIRS = {"target", ".git", "node_modules", "generated", "docs"}
CODE_SUFFIXES = {".rs": "max_file_lines", ".js": "max_script_lines", ".css": "max_file_lines",
                 ".html": "max_file_lines", ".py": "max_script_lines", ".sql": "max_file_lines"}


def limits() -> dict[str, int]:
    with CONTRACTS.open("rb") as fh:
        return tomllib.load(fh)["limits"]


def scan() -> list[tuple[Path, int, int]]:
    cfg = limits()
    offenders: list[tuple[Path, int, int]] = []
    for path in ROOT.rglob("*"):
        if not path.is_file() or path.suffix not in CODE_SUFFIXES:
            continue
        if any(part in SKIP_DIRS for part in path.relative_to(ROOT).parts):
            continue
        cap = cfg[CODE_SUFFIXES[path.suffix]]
        lines = sum(1 for _ in path.open("rb"))
        if lines > cap:
            offenders.append((path.relative_to(ROOT), lines, cap))
    return offenders


def run() -> int:
    offenders = scan()
    for path, lines, cap in offenders:
        print(f"  ✘ {path}: {lines} سطرًا (السقف {cap}) — قسّم الملف بحسب التخصص.")
    if offenders:
        return 1
    print(f"  ✔ كل الملفات داخل الحدود ({len(CODE_SUFFIXES)} أنواع ملفات)")
    return 0
