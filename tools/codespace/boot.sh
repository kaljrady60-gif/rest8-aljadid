#!/usr/bin/env bash
# تهيئة المشروع داخل GitHub Codespaces: تنزيل الكود، بناء، جسر تحكم، أنفاق.
# الاستخدام:  bash /tmp/boot.sh "<رابط_الحزمة>"
set -uo pipefail

TARBALL_URL="${1:-}"
PROJ="$HOME/smart-accountant"
BRIDGE_PORT=8787
APP_PORT=3300
LOGS="$HOME/.bridge-logs"
mkdir -p "$LOGS"

say() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
ok()  { printf '\033[1;32m✓ %s\033[0m\n' "$*"; }
bad() { printf '\033[1;31m✗ %s\033[0m\n' "$*"; }

if [ -z "$TARBALL_URL" ]; then
  bad "رابط الحزمة مفقود. الصيغة: bash /tmp/boot.sh \"https://...\""
  exit 1
fi

say "١/٧ تجهيز أدوات النظام"
if command -v sudo >/dev/null 2>&1; then
  (sudo -n apt-get update -qq && sudo -n apt-get install -y -qq build-essential pkg-config libssl-dev curl git tar gzip >/dev/null 2>&1) \
    && ok "الحزم النظامية جاهزة" || bad "تعذّر تثبيت بعض الحزم (سنكمل؛ غالبًا غير لازمة في Codespaces)"
else
  ok "لا sudo — نفترض الأدوات مثبّتة مسبقًا"
fi

say "٢/٧ التأكد من Rust"
if ! command -v cargo >/dev/null 2>&1; then
  if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
fi
if ! command -v cargo >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable >"$LOGS/rustup.log" 2>&1
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
fi
if command -v cargo >/dev/null 2>&1; then ok "cargo: $(cargo --version)"; else bad "فشل تثبيت Rust — التفاصيل في $LOGS/rustup.log"; fi

say "٣/٧ التأكد من cloudflared"
if ! command -v cloudflared >/dev/null 2>&1; then
  ARCH=$(uname -m)
  case "$ARCH" in
    x86_64|amd64) CF=cloudflared-linux-amd64 ;;
    aarch64|arm64) CF=cloudflared-linux-arm64 ;;
    *) CF=cloudflared-linux-amd64 ;;
  esac
  curl -fsSL "https://github.com/cloudflare/cloudflared/releases/latest/download/$CF" -o /tmp/cloudflared \
    && chmod +x /tmp/cloudflared \
    && (sudo -n mv /tmp/cloudflared /usr/local/bin/cloudflared 2>/dev/null || mv /tmp/cloudflared "$HOME/.local/bin/cloudflared" 2>/dev/null || true)
  export PATH="$HOME/.local/bin:$PATH"
fi
command -v cloudflared >/dev/null 2>&1 && ok "cloudflared جاهز" || bad "cloudflared غير متاح — النفق لن يعمل"

say "٤/٧ تنزيل المشروع"
if [ -d "$PROJ/src" ]; then
  ok "المشروع موجود مسبقًا — أستبدله بالنسخة الجديدة"
fi
rm -rf "$PROJ" && mkdir -p "$PROJ"
if curl -fsSL "$TARBALL_URL" -o /tmp/proj.tar.gz && tar -xzf /tmp/proj.tar.gz -C "$PROJ"; then
  ok "استُخرج الكود: $(find "$PROJ/src" -name '*.rs' | wc -l) ملف Rust"
else
  bad "فشل تنزيل الحزمة"
  exit 1
fi

say "٥/٧ تشغيل الجسر"
BRIDGE_TOKEN=$(python3 -c 'import secrets;print(secrets.token_urlsafe(18))')
export BRIDGE_TOKEN BRIDGE_PORT
pkill -f "bridge.py" 2>/dev/null
setsid nohup python3 "$PROJ/tools/codespace/bridge.py" >"$LOGS/bridge.log" 2>&1 &
sleep 2
curl -s -m 5 "http://127.0.0.1:$BRIDGE_PORT/health" | grep -q '"ok":true' && ok "الجسر يعمل على $BRIDGE_PORT" || bad "الجسر لم يبدأ"

say "٦/٧ فتح الأنفاق"
tunnel_url() {
  local port="$1" log="$LOGS/tunnel-$1.log"
  : >"$log"
  setsid nohup cloudflared tunnel --url "http://localhost:$port" --no-autoupdate --loglevel info >"$log" 2>&1 &
  for _ in $(seq 1 60); do
    url=$(grep -oE 'https://[a-z0-9-]+\.trycloudflare\.com' "$log" | head -1)
    [ -n "$url" ] && { echo "$url"; return 0; }
    sleep 1
  done
  return 1
}
BRIDGE_URL=$(tunnel_url "$BRIDGE_PORT") && ok "نفق الجسر: $BRIDGE_URL" || bad "نفق الجسر فشل"
APP_URL=$(tunnel_url "$APP_PORT") && ok "نفق التطبيق: $APP_URL" || bad "نفق التطبيق فشل"

say "٧/٧ بناء المشروع (يعمل في الخلفية)"
(cd "$PROJ" && cargo build --release) >"$LOGS/build.log" 2>&1 &
BUILD_PID=$!
echo "البناء يعمل — PID $BUILD_PID — السجل: $LOGS/build.log"

cat >"$HOME/bridge-info.txt" <<INFO
BRIDGE_URL=$BRIDGE_URL
BRIDGE_TOKEN=$BRIDGE_TOKEN
APP_URL=$APP_URL
BUILD_PID=$BUILD_PID
INFO

printf '\n\033[1;33m======= انسخ الأسطر الثلاثة التالية وأرسلها في المحادثة =======\033[0m\n'
echo "BRIDGE_URL=$BRIDGE_URL"
echo "BRIDGE_TOKEN=$BRIDGE_TOKEN"
echo "APP_URL=$APP_URL"
printf '\033[1;33m=================================================================\033[0m\n'
echo "ملف بالمعلومات نفسها: $HOME/bridge-info.txt"
echo "لمتابعة البناء: tail -f $LOGS/build.log"
