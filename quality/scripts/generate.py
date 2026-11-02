#!/usr/bin/env python3
"""مُولِّد العقود الموحّدة: contracts.toml ← Rust + JavaScript + HTML.

الاستخدام:
    python3 quality/scripts/generate.py          # توليد
    python3 quality/scripts/generate.py --check  # فحص التطابق فقط (للبوابات)

لا يعدّل هذا الملف أي شيء آخر، ولا يقرأ غير الملف الموحّد.
"""
from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "quality" / "contracts.toml"
RUST_OUT = ROOT / "src" / "generated" / "contracts.rs"
JS_OUT = ROOT / "assets" / "scripts" / "00-contracts.js"
HTML_OUT = ROOT / "templates" / "components" / "contracts.html"


def load() -> dict:
    with SOURCE.open("rb") as fh:
        return tomllib.load(fh)


def rust_str(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def emit_rust(c: dict) -> str:
    note = c["generated_note"]
    lines: list[str] = [
        f"//! {note}",
        "#![allow(dead_code)]",
        "",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub struct CurrencySpec {",
        "    pub code: &'static str,",
        "    pub label: &'static str,",
        "    pub iso: bool,",
        "    pub decimals: u32,",
        "}",
        "",
        "pub const CURRENCIES: &[CurrencySpec] = &[",
    ]
    for item in c["currencies"]["items"]:
        lines.append(
            "    CurrencySpec { code: %s, label: %s, iso: %s, decimals: %d },"
            % (
                rust_str(item["code"]),
                rust_str(item["label"]),
                "true" if item["iso"] else "false",
                item["decimals"],
            )
        )
    lines += ["];", "", "pub fn supported_currency(code: &str) -> bool {",
              "    CURRENCIES.iter().any(|c| c.code == code)", "}", ""]

    limits = c["limits"]
    lines.append("pub mod limits {")
    for key, value in limits.items():
        lines.append(f"    pub const {key.upper()}: usize = {value};")
    lines += ["}", ""]

    sec = c["security"]
    lines.append("pub mod security {")
    for key, value in sec.items():
        lines.append(f"    pub const {key.upper()}: i64 = {value};")
    lines += ["}", ""]

    for group in ("account", "funds", "periods"):
        lines.append(f"pub mod {group} {{")
        for key, values in c[group].items():
            const = key.upper()
            body = ", ".join(rust_str(v) for v in values)
            lines.append(f"    pub const {const}: &[&str] = &[{body}];")
        lines += ["}", ""]

    lines.append("pub mod roles {")
    order = c["roles"]["order"]
    lines.append("    pub const ORDER: &[&str] = &[%s];" % ", ".join(rust_str(r) for r in order))
    lines.append("    pub fn rank(role: &str) -> Option<usize> {")
    lines.append("        ORDER.iter().position(|r| *r == role)")
    lines.append("    }")
    lines.append("    pub fn at_least(role: &str, required: &str) -> bool {")
    lines.append("        matches!((rank(role), rank(required)), (Some(a), Some(b)) if a >= b)")
    lines.append("    }")
    labels = ", ".join(
        f"({rust_str(k)}, {rust_str(v)})" for k, v in c["roles"]["labels"].items()
    )
    lines.append(f"    pub const LABELS: &[(&str, &str)] = &[{labels}];")
    lines.append("    pub fn label(role: &str) -> &'static str {")
    lines.append("        LABELS")
    lines.append("            .iter()")
    lines.append("            .find(|(r, _)| *r == role)")
    lines.append("            .map(|(_, l)| *l)")
    lines.append("            .unwrap_or(\"غير معروف\")")
    lines.append("    }")
    lines += ["}", ""]

    lines.append("#[derive(Debug, Clone, Copy)]")
    lines.append("pub struct ScreenSpec {")
    lines.append("    pub id: &'static str,")
    lines.append("    pub title: &'static str,")
    lines.append("    pub api: &'static str,")
    lines.append("}")
    lines.append("pub const SCREENS: &[ScreenSpec] = &[")
    for s in c["screens"]["implemented"]:
        lines.append(
            "    ScreenSpec { id: %s, title: %s, api: %s },"
            % (rust_str(s["id"]), rust_str(s["title"]), rust_str(s["api"]))
        )
    lines += ["];", ""]

    lines.append("#[derive(Debug, Clone, Copy)]")
    lines.append("pub struct ReportSpec {")
    lines.append("    pub id: &'static str,")
    lines.append("    pub title: &'static str,")
    lines.append("    pub kind: &'static str,")
    lines.append("}")
    lines.append("pub const REPORTS: &[ReportSpec] = &[")
    for r in c["reports"]["items"]:
        lines.append(
            "    ReportSpec { id: %s, title: %s, kind: %s },"
            % (rust_str(r["id"]), rust_str(r["title"]), rust_str(r["kind"]))
        )
    lines += ["];", "", "pub const SCHEMA_VERSION: u32 = %d;" % c["schema_version"], ""]
    return "\n".join(lines)


def emit_js(c: dict) -> str:
    import json

    payload = {
        "schemaVersion": c["schema_version"],
        "limits": c["limits"],
        "security": c["security"],
        "roles": c["roles"],
        "currencies": c["currencies"]["items"],
        "screens": c["screens"]["implemented"],
        "reports": c["reports"]["items"],
        "account": c["account"],
        "funds": c["funds"],
        "periods": c["periods"],
    }
    body = json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True)
    return (
        f"// {c['generated_note']}\n"
        "window.CONTRACTS = Object.freeze(%s);\n"
        "window.CONTRACT_SCREENS = window.CONTRACTS.screens;\n"
        "window.CONTRACT_CURRENCIES = window.CONTRACTS.currencies;\n" % body
    )


def emit_html(c: dict) -> str:
    screens = " ".join(s["id"] for s in c["screens"]["implemented"])
    return (
        f"<!-- {c['generated_note']} -->\n"
        '<meta name="contracts-version" content="%d">\n'
        '<meta name="contracts-screens" content="%s">\n' % (c["schema_version"], screens)
    )


def formatted(path: Path, content: str) -> str:
    """يعيد المحتوى بعد تطبيق التنسيق نفسه الذي يخضع له الملف على القرص."""
    if path.suffix != ".rs" or not shutil.which("rustfmt"):
        return content
    import tempfile

    with tempfile.NamedTemporaryFile("w", suffix=".rs", delete=False, encoding="utf-8") as fh:
        fh.write(content)
        temp = Path(fh.name)
    subprocess.run(["rustfmt", "--edition", "2024", str(temp)], check=False)
    result = temp.read_text(encoding="utf-8")
    temp.unlink(missing_ok=True)
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    contracts = load()
    targets = {
        RUST_OUT: emit_rust(contracts),
        JS_OUT: emit_js(contracts),
        HTML_OUT: emit_html(contracts),
    }
    stale = []
    for path, content in targets.items():
        if args.check:
            current = path.read_text(encoding="utf-8") if path.exists() else ""
            if current != formatted(path, content):
                stale.append(path.relative_to(ROOT))
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        if path.suffix == ".rs" and shutil.which("rustfmt"):
            # نُنسّق الملف المولّد حتى لا تتعارض بوابة التنسيق مع بوابة العقود.
            subprocess.run(["rustfmt", "--edition", "2024", str(path)], check=False)
        print(f"✔ {path.relative_to(ROOT)}")
    if args.check:
        if stale:
            print("✘ عقود غير متطابقة: " + ", ".join(str(p) for p in stale))
            print("  نفّذ: python3 quality/scripts/generate.py")
            return 1
        print("✔ العقود متطابقة مع الملف الموحّد")
    return 0


if __name__ == "__main__":
    sys.exit(main())
