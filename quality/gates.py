#!/usr/bin/env python3
"""بوابة الجودة الموحّدة — نقطة دخول واحدة لكل الفحوص.

    python3 quality/gates.py            # كل الفحوص السريعة
    python3 quality/gates.py size       # فحص واحد بالاسم
    python3 quality/gates.py --list     # قائمة الفحوص

كل فحص يرجع 0 (نجاح) أو 1 (فشل)، ويطبع سطرًا واحدًا لكل مخالفة.
الفحوص الثقيلة (تغطية/طفرات) تُشغَّل بأمر مستقل عبر `quality/scripts/`.
"""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

from checks import contracts_sync, file_size, hygiene, python_scripts, rust  # noqa: E402

CHECKS = {
    "gen": ("توليد العقود من الملف الموحّد", lambda: contracts_sync.generate()),
    "contracts": ("تطابق المُولَّد مع contracts.toml", contracts_sync.run),
    "size": ("حدود حجم الملفات والسكربتات", file_size.run),
    "hygiene": ("نظافة المستودع وأنماط ممنوعة", hygiene.run),
    "fmt": ("تنسيق Rust", lambda: rust.cargo("fmt", ["--all", "--", "--check"])),
    "clippy": ("تحليل Rust الصارم", lambda: rust.clippy()),
    "test": ("اختبارات Rust (nextest)", lambda: rust.nextest()),
    "deps": ("تبعيات غير مستخدمة/غير آمنة", lambda: rust.deps()),
    "py": ("فحص سكربتات Python (ruff)", python_scripts.run),
}
FAST = ["contracts", "size", "hygiene", "py"]


def main(argv: list[str]) -> int:
    if "--list" in argv:
        for name, (label, _) in CHECKS.items():
            print(f"  {name:<10} {label}")
        return 0
    names = [a for a in argv if not a.startswith("-")] or FAST
    failed: list[str] = []
    for name in names:
        if name not in CHECKS:
            print(f"✘ فحص غير معروف: {name}")
            return 2
        label, fn = CHECKS[name]
        print(f"▶ {label} …")
        try:
            code = fn()
        except subprocess.CalledProcessError as exc:
            code = exc.returncode or 1
        if code == 0:
            print(f"✔ {label}")
        else:
            print(f"✘ {label}")
            failed.append(name)
    if failed:
        print("فشل: " + ", ".join(failed))
        return 1
    print("كل البوابات ناجحة ✔")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
