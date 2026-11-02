"""أغلفة رقيقة حول أدوات Rust المعتمدة (cargo / nextest / deny / machete)."""
from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def _env() -> dict[str, str]:
    env = dict(os.environ)
    cargo_home = Path(env.get("CARGO_HOME") or Path.home() / ".cargo")
    env["PATH"] = f"{cargo_home / 'bin'}{os.pathsep}{env.get('PATH', '')}"
    return env


def which(tool: str) -> bool:
    return shutil.which(tool, path=_env()["PATH"]) is not None


def cargo(sub: str, args: list[str]) -> int:
    return subprocess.run(
        ["cargo", sub, *args], cwd=ROOT, env=_env(), check=False
    ).returncode


def clippy() -> int:
    return cargo("clippy", ["--all-targets", "--", "-D", "warnings"])


def nextest() -> int:
    if not which("cargo-nextest"):
        print("  ! cargo-nextest غير مثبّت — استخدام cargo test")
        return cargo("test", ["--release"])
    return subprocess.run(
        ["cargo", "nextest", "run", "--release"], cwd=ROOT, env=_env(), check=False
    ).returncode


def deps() -> int:
    code = 0
    if which("cargo-machete"):
        code |= subprocess.run(["cargo", "machete"], cwd=ROOT, env=_env(), check=False).returncode
    if which("cargo-deny"):
        code |= subprocess.run(
            ["cargo", "deny", "--all-features", "check"], cwd=ROOT, env=_env(), check=False
        ).returncode
    return code
