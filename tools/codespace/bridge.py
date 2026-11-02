#!/usr/bin/env python3
"""جسر التحكم البعيد — يعمل داخل مساحة GitHub Codespaces.

الفكرة: عملية واحدة صغيرة (مكتبة بايثون القياسية فقط) تكشف واجهة HTTP محمية
برمز سرّي، تتيح: تنفيذ أوامر، رفع/تنزيل ملفات، فتح أنفاق مؤقتة لأي منفذ،
وقراءة سجلات العمليات الجارية. لا اعتماد على أي حزمة خارجية.
"""
from __future__ import annotations

import base64
import json
import os
import secrets
import shutil
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

TOKEN = os.environ.get("BRIDGE_TOKEN") or secrets.token_urlsafe(18)
HOME = Path.home()
LOG_DIR = HOME / ".bridge-logs"
LOG_DIR.mkdir(exist_ok=True)
TUNNELS: dict[int, dict] = {}
LOCK = threading.Lock()
MAX_OUT = 40_000


def tail(path: Path, n: int = 200) -> str:
    try:
        data = path.read_text(errors="replace").splitlines()
    except OSError:
        return ""
    return "\n".join(data[-n:])


def cloudflared_path() -> str | None:
    """يبحث عن cloudflared في PATH ثم في المواضع المعتادة."""
    found = shutil.which("cloudflared")
    if found:
        return found
    for candidate in (HOME / "cloudflared", HOME / ".local/bin/cloudflared",
                      Path("/usr/local/bin/cloudflared"), Path("/usr/bin/cloudflared")):
        if candidate.is_file():
            return str(candidate)
    return None


def start_tunnel(port: int) -> dict:
    """يفتح نفق cloudflared سريعًا ويعيد رابطه العام."""
    with LOCK:
        known = TUNNELS.get(port)
        if known and known.get("url"):
            return known
    log = LOG_DIR / f"tunnel-{port}.log"
    log.write_text("")
    binary = cloudflared_path()
    if not binary:
        return {"port": port, "url": None, "error": "cloudflared غير مثبّت"}
    proc = subprocess.Popen(
        [binary, "tunnel", "--url", f"http://localhost:{port}",
         "--no-autoupdate", "--loglevel", "info"],
        stdout=log.open("wb"), stderr=subprocess.STDOUT,
    )
    url = None
    for _ in range(120):
        text = log.read_text(errors="replace")
        for line in text.splitlines():
            if "trycloudflare.com" in line:
                for word in line.replace("|", " ").split():
                    if word.startswith("https://") and "trycloudflare.com" in word:
                        url = word.strip()
                        break
        if url:
            break
        time.sleep(0.5)
    info = {"port": port, "pid": proc.pid, "url": url, "log": str(log)}
    with LOCK:
        TUNNELS[port] = info
    return info


class Handler(BaseHTTPRequestHandler):
    server_version = "Bridge/2"

    def log_message(self, fmt, *args):  # أقل ضجيجًا
        pass

    # ---------- أدوات ----------
    def _send(self, code: int, payload) -> None:
        body = payload if isinstance(payload, bytes) else json.dumps(
            payload, ensure_ascii=False).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json; charset=utf-8"
                         if not isinstance(payload, bytes) else "application/octet-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _auth(self) -> bool:
        if self.headers.get("X-Token") == TOKEN:
            return True
        self._send(401, {"error": "رمز غير صحيح"})
        return False

    def _body(self) -> dict:
        length = int(self.headers.get("Content-Length") or 0)
        if not length:
            return {}
        try:
            return json.loads(self.rfile.read(length) or b"{}")
        except json.JSONDecodeError:
            return {}

    # ---------- مسارات ----------
    def do_GET(self):  # noqa: N802
        parsed = urlparse(self.path)
        query = parse_qs(parsed.query)
        if parsed.path == "/health":
            return self._send(200, {"ok": True, "host": os.uname().nodename})
        if not self._auth():
            return
        if parsed.path == "/ports":
            out = run(["bash", "-lc", "ss -ltnp 2>/dev/null | awk 'NR>1{print $4}'"])
            return self._send(200, {"ports": out["out"]})
        if parsed.path == "/log":
            path = (query.get("path") or [""])[0]
            n = int((query.get("n") or ["200"])[0])
            return self._send(200, {"tail": tail(Path(path), n)})
        if parsed.path == "/get":
            path = Path((query.get("path") or [""])[0])
            if not path.is_file():
                return self._send(404, {"error": "لا يوجد ملف"})
            return self._send(200, {"b64": base64.b64encode(path.read_bytes()).decode()})
        if parsed.path == "/tunnels":
            return self._send(200, {"tunnels": TUNNELS})
        return self._send(404, {"error": "مسار غير معروف"})

    def do_POST(self):  # noqa: N802
        if not self._auth():
            return
        parsed = urlparse(self.path)
        data = self._body()

        if parsed.path == "/run":
            cmd = (data.get("cmd") or "").strip()
            if not cmd:
                return self._send(400, {"error": "أمر فارغ"})
            cwd = data.get("cwd") or str(HOME)
            if data.get("background"):
                log = LOG_DIR / f"bg-{int(time.time())}.log"
                proc = subprocess.Popen(["bash", "-lc", cmd], cwd=cwd,
                                        stdout=log.open("wb"), stderr=subprocess.STDOUT,
                                        start_new_session=True)
                return self._send(200, {"pid": proc.pid, "log": str(log)})
            res = run(["bash", "-lc", cmd], cwd=cwd,
                      timeout=int(data.get("timeout") or 240))
            return self._send(200, res)

        if parsed.path == "/put":
            path = Path(data["path"])
            if not data.get("b64") and data.get("text") is not None:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(data["text"])
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(base64.b64decode(data["b64"]))
            if data.get("mode"):
                path.chmod(int(data["mode"], 8))
            return self._send(200, {"ok": True, "size": path.stat().st_size})

        if parsed.path == "/tunnel":
            port = int(data.get("port") or 0)
            if not port:
                return self._send(400, {"error": "منفذ مطلوب"})
            if not cloudflared_path():
                return self._send(500, {"error": "cloudflared غير مثبّت"})
            return self._send(200, start_tunnel(port))

        return self._send(404, {"error": "مسار غير معروف"})


def run(argv, cwd: str | None = None, timeout: int = 240) -> dict:
    try:
        proc = subprocess.run(argv, cwd=cwd, capture_output=True, text=True,
                              timeout=timeout)
        out = (proc.stdout or "") + (proc.stderr or "")
        return {"code": proc.returncode, "out": out[-MAX_OUT:]}
    except subprocess.TimeoutExpired as exc:
        partial = (exc.stdout or "") if isinstance(exc.stdout, str) else ""
        return {"code": 124, "out": (partial + "\n[انتهت المهلة]")[-MAX_OUT:]}


def main() -> None:
    port = int(os.environ.get("BRIDGE_PORT") or 8787)
    server = ThreadingHTTPServer(("0.0.0.0", port), Handler)
    print(f"BRIDGE_TOKEN={TOKEN}", flush=True)
    print(f"BRIDGE_PORT={port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
