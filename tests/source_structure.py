#!/usr/bin/env python3
"""Check the project's physical source-file limit without treating data as code."""
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent.parent
SOURCE_DIRS = ("src", "assets", "templates", "tests", "tools")
SOURCE_SUFFIXES = {".rs", ".js", ".cjs", ".css", ".py", ".html", ".sh"}
MAX_LINES = 400


def main():
    files = sorted(
        file
        for directory in SOURCE_DIRS
        for file in (ROOT / directory).rglob("*")
        if file.is_file() and file.suffix in SOURCE_SUFFIXES
    )
    oversized = []
    long_lines = []
    for file in files:
        lines = file.read_text(encoding="utf-8").splitlines()
        if len(lines) > MAX_LINES:
            oversized.append((file.relative_to(ROOT), len(lines)))
        count = sum(len(line) > 300 for line in lines)
        if count:
            long_lines.append((file.relative_to(ROOT), count))
    for path, count in oversized:
        print(f"FAIL: {path} has {count} lines; limit is {MAX_LINES}")
    if oversized:
        return 1
    print(f"PASS: {len(files)} source files have at most {MAX_LINES} lines")
    if long_lines:
        print("WARNING: long lines still require readability review (not a size-limit pass):")
        for path, count in long_lines:
            print(f"  {path}: {count} lines over 300 characters")
    return 0


if __name__ == "__main__":
    sys.exit(main())
