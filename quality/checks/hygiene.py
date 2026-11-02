"""حارس النظافة: أنماط ممنوعة في الشيفرة، بقاعدة واحدة قابلة للتوسيع.

كل قاعدة هي (المعرّف، الوصف، الامتدادات، النمط، الملفات المستثناة).
الاستثناء الموضعي: ضع التعليق `gates: allow <rule>` في السطر نفسه أو السطر السابق.
"""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

Rule = tuple[str, str, set[str], re.Pattern[str], tuple[str, ...]]

RULES: list[Rule] = [
    (
        "unsafe",
        "استخدام unsafe ممنوع في هذا المشروع",
        {".rs"},
        re.compile(r"\bunsafe\s*\{"),
        (),
    ),
    (
        "float",
        "أنواع عائمة ممنوعة في مسار المال (استخدم Decimal)",
        {".rs"},
        re.compile(r"\b(f32|f64)\b"),
        (),
    ),
    (
        "unwrap",
        "unwrap()/expect() ممنوع خارج الاختبارات (استخدم ? أو خطأ صريح)",
        {".rs"},
        re.compile(r"\.(unwrap|expect)\("),
        (),
    ),
    (
        "marker",
        "TODO/FIXME بلا خطة مكتوبة (اكتب TODO(issue):)",
        {".rs", ".js", ".css", ".py"},
        re.compile(r"\b(TODO|FIXME|XXX)\b(?!\()"),
        (),
    ),
    (
        "legacy-api",
        "مسارات API قديمة بلا عزل شركة — انقلها إلى /api/finance/*",
        {".rs"},
        re.compile(r'"/api/(drafts|vouchers|periods|ledger|accounts)"'),
        (),
    ),
]

ALLOW = re.compile(r"gates:\s*allow\s+([a-z-]+)")


def _allowed(name: str, lines: list[str], index: int) -> bool:
    window = lines[max(0, index - 1): index + 1]
    return any(name in ALLOW.findall(line) for line in window)


def scan() -> list[str]:
    problems: list[str] = []
    for path in ROOT.rglob("*"):
        if not path.is_file() or path.suffix not in {s for r in RULES for s in r[2]}:
            continue
        rel = path.relative_to(ROOT)
        # ملفات الاختبارات مستثناة من قواعد الجودة الصارمة: الاختبار يفشل صراحةً.
        if any(part in {"target", "generated", ".git", "node_modules", "tests", "api_tests"} for part in rel.parts):
            continue
        stem = path.stem
        # ملفات الاختبارات الفردية (tests.rs)وسلوكها مستثناة أيضًا.
        if stem.startswith("test") or stem.endswith("tests") or stem.endswith("support"):
            continue
        # ملف تعريف القواعد يستخدم أسماء الأنماط نفسها؛ لا يُفحص ضدّها.
        if rel.as_posix().startswith("quality/checks/"):
            continue
        lines = path.read_text(encoding="utf-8", errors="ignore").splitlines()
        for name, message, suffixes, pattern, skipped in RULES:
            if path.suffix not in suffixes or any(key in stem for key in skipped):
                continue
            for number, line in enumerate(lines, start=1):
                if pattern.search(line) and not _allowed(name, lines, number - 1):
                    problems.append(f"  ✘ {rel}:{number} [{name}] {message}")
    return problems


def run() -> int:
    problems = scan()
    for line in problems:
        print(line)
    if problems:
        print(f"  المجموع: {len(problems)} مخالفة")
        return 1
    print(f"  ✔ لا مخالفات في {len(RULES)} قاعدة")
    return 0
