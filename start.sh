#!/usr/bin/env bash
# المحاسب الذكي — تشغيل بنقرة واحدة داخل Google Cloud Shell.
# الاستخدام:  bash ~/smart-accountant/start.sh
set -u
APP="$HOME/smart-accountant"
PORT=3300
CF="$HOME/cloudflared"

cd "$APP" || { echo "المشروع غير موجود في $APP"; exit 1; }

echo "== 1/4 التحقق من الملف التنفيذي =="
if [ ! -x bin/smart-accountant ]; then
  echo "لا يوجد ملف تنفيذي — سأبنيه الآن (يستغرق دقائق)…"
  CARGO_TARGET_DIR=/tmp/target cargo build --release || { echo "فشل البناء"; exit 1; }
  mkdir -p bin && cp -f /tmp/target/release/smart-accountant bin/smart-accountant
fi
echo "الملف التنفيذي جاهز: $(du -h bin/smart-accountant | cut -f1)"

echo "== 2/4 تهيئة قاعدة البيانات (مرة واحدة) =="
mkdir -p data
export SMART_DB="$APP/data/smart.sqlite"
if [ ! -f "$SMART_DB" ]; then
  printf 'خالد\n1\n1\n' | script -qec "SMART_DEV_WEAK_PASSWORD=1 SMART_DB=$SMART_DB $APP/bin/smart-accountant --init-admin" /dev/null >/dev/null 2>&1
  "$APP/bin/smart-accountant" --init-company "شركتي" >/dev/null 2>&1
  "$APP/bin/smart-accountant" --seed-demo >/dev/null 2>&1
  echo "أُنشئت القاعدة والحساب والبيانات التجريبية."
else
  echo "القاعدة موجودة — لا تغيير."
fi

echo "== 3/4 تشغيل الخدمة على المنفذ $PORT =="
fuser -k -n tcp "$PORT" >/dev/null 2>&1
sleep 1
# وضع المعاينة: كوكي مقسّم (SameSite=None; Partitioned) وجلسة بالترويسة
# لأن iframe المعاينة عبر المواقع قد يحجب الكوكي كليًا.
SMART_DB="$SMART_DB" SMART_BIND="0.0.0.0:$PORT" SMART_SECURE_COOKIE=1 \
  SMART_PREVIEW_PARTITIONED_COOKIE=1 SMART_PREVIEW_HEADER_SESSION=1 \
  setsid nohup "$APP/bin/smart-accountant" >> "$APP/data/run.log" 2>&1 </dev/null &
sleep 3
if ! curl -fsS -m 5 -o /dev/null "http://127.0.0.1:$PORT/"; then
  echo "الخدمة لم تستجب — راجع $APP/data/run.log"
  tail -5 "$APP/data/run.log"
  exit 1
fi
echo "الخدمة تعمل محليًا على http://127.0.0.1:$PORT"

echo "== 4/4 نفق عام =="
URL=""
if [ -x "$CF" ]; then
  if ! pgrep -f "cloudflared tunnel --url http://127.0.0.1:$PORT" >/dev/null 2>&1; then
    : > /tmp/cf-app.log
    setsid nohup "$CF" tunnel --url "http://127.0.0.1:$PORT" --no-autoupdate --protocol http2 \
      >> /tmp/cf-app.log 2>&1 </dev/null &
    for _ in $(seq 1 20); do
      sleep 2
      URL=$(grep -oE 'https://[a-z0-9-]+\.trycloudflare\.com' /tmp/cf-app.log | tail -1)
      [ -n "$URL" ] && break
    done
  else
    URL=$(grep -oE 'https://[a-z0-9-]+\.trycloudflare\.com' /tmp/cf-app.log 2>/dev/null | tail -1)
  fi
fi

echo
echo "════════════════════════════════════════════"
echo "  الرابط العام : ${URL:-غير متاح — استخدم Web Preview على المنفذ $PORT}"
echo "  الدخول       : خالد / 1"
echo "  المجلد       : $APP"
echo "════════════════════════════════════════════"
