#!/usr/bin/env bash
# تشغيل/إعادة تشغيل المنظومة داخل Codespaces: التطبيق + الأنفاق + نبضة كل دقيقة.
# الاستخدام:  bash ~/smart-accountant/tools/codespace/run.sh
set -uo pipefail
PROJ="$HOME/smart-accountant"
LOGS="$HOME/.bridge-logs"
APP_PORT=3300
BRIDGE_PORT=8787
mkdir -p "$LOGS"

kill_port() {
  local port="$1"
  local pid
  pid=$(ss -ltnp 2>/dev/null | grep ":$port" | grep -oE 'pid=[0-9]+' | cut -d= -f2 | head -1)
  [ -n "${pid:-}" ] && kill "$pid" 2>/dev/null && sleep 1
  return 0
}

start_tunnel() {
  local port="$1" log="$LOGS/tunnel-$1.log"
  if ss -ltnp 2>/dev/null | grep -q ":$port"; then :; fi
  if grep -q "trycloudflare.com" "$log" 2>/dev/null && pgrep -f "tunnel --url http://localhost:$port" >/dev/null; then
    grep -oE 'https://[a-z0-9-]+\.trycloudflare\.com' "$log" | head -1
    return 0
  fi
  : >"$log"
  setsid nohup cloudflared tunnel --url "http://localhost:$port" --no-autoupdate \
    --loglevel info >"$log" 2>&1 &
  for _ in $(seq 1 60); do
    local url
    url=$(grep -oE 'https://[a-z0-9-]+\.trycloudflare\.com' "$log" | head -1)
    [ -n "$url" ] && { echo "$url"; return 0; }
    sleep 1
  done
  return 1
}

echo "==> التطبيق"
kill_port "$APP_PORT"
cd "$PROJ" || exit 1
SMART_DB="$PROJ/data/smart.sqlite" SMART_BIND="0.0.0.0:$APP_PORT" \
  SMART_SECURE_COOKIE=1 SMART_EXPERIMENTAL_POSTING=1 \
  setsid nohup ./target/release/smart-accountant >"$LOGS/app.log" 2>&1 < /dev/null &
for _ in $(seq 1 30); do
  curl -s -o /dev/null "http://127.0.0.1:$APP_PORT/" && break
  sleep 1
done
curl -s -o /dev/null -w "التطبيق محليًا: %{http_code}\n" "http://127.0.0.1:$APP_PORT/"

echo "==> الجسر"
if curl -s -m 3 "http://127.0.0.1:$BRIDGE_PORT/health" | grep -q '"ok":true'; then
  echo "الجسر يعمل مسبقًا"
else
  export BRIDGE_TOKEN="${BRIDGE_TOKEN:-$(python3 -c 'import secrets;print(secrets.token_urlsafe(18))')}"
  export BRIDGE_PORT
  setsid nohup python3 "$PROJ/tools/codespace/bridge.py" >"$LOGS/bridge.log" 2>&1 < /dev/null &
  sleep 3
  grep -oE 'BRIDGE_TOKEN=.*' "$LOGS/bridge.log" | head -1
fi

echo "==> الأنفاق"
BRIDGE_URL=$(start_tunnel "$BRIDGE_PORT") || BRIDGE_URL="(فشل)"
APP_URL=$(start_tunnel "$APP_PORT") || APP_URL="(فشل)"

echo "==> نبضة كل دقيقة (تمنع خمول المساحة)"
if ! pgrep -f "keepalive.sh" >/dev/null; then
  cat >"$PROJ/tools/codespace/keepalive.sh" <<'KA'
#!/usr/bin/env bash
while true; do
  date -u +"%Y-%m-%dT%H:%M:%SZ" >> "$HOME/.bridge-logs/keepalive.log"
  curl -s -o /dev/null -m 10 http://127.0.0.1:3300/ || true
  sleep 60
done
KA
  chmod +x "$PROJ/tools/codespace/keepalive.sh"
  setsid nohup bash "$PROJ/tools/codespace/keepalive.sh" >/dev/null 2>&1 < /dev/null &
fi

cat <<INFO

================ الروابط ================
التطبيق:   $APP_URL
الجسر:     $BRIDGE_URL
الرمز:     ${BRIDGE_TOKEN:-(محفوظ من التشغيل الأول)}
السجلات:   $LOGS
=========================================
INFO
