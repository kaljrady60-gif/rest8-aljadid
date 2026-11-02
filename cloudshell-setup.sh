#!/usr/bin/env bash
# تشغيل «المحاسب الذكي» على Google Cloud Shell أو أي جهاز Linux بلا خبرة سابقة.
# الاستخدام:  bash cloudshell-setup.sh
set -euo pipefail

echo "== 1/5 تجهيز أدوات البناء =="
if ! command -v cargo >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
  sh /tmp/rustup-init.sh -y --profile minimal
fi
# shellcheck disable=SC1091
. "$HOME/.cargo/env"

echo "== 2/5 بناء نسخة الإنتاج =="
cd "$(dirname "$0")"
cargo build --release

echo "== 3/5 تهيئة قاعدة البيانات والحساب =="
mkdir -p data
export SMART_DB="$PWD/data/smart.sqlite"
if [ ! -f "$SMART_DB" ]; then
  SMART_DEV_WEAK_PASSWORD=1 ./target/release/smart-accountant --init-admin
  ./target/release/smart-accountant --init-company "شركتي"
  ./target/release/smart-accountant --seed-demo
fi

echo "== 4/5 تشغيل الخدمة على المنفذ 3300 =="
cat <<'TIP'

شغّل الخدمة بالأمر التالي (أو دعه يعمل في الخلفية بـ nohup):

  SMART_DB="$PWD/data/smart.sqlite" SMART_BIND=0.0.0.0:3300 \
  ./target/release/smart-accountant

ثم من قائمة Cloud Shell:  Web Preview → Preview on port 3300.
الدخول الافتراضي: المستخدم «خالد» وكلمة المرور «1» (غيّرها فورًا من الإعدادات).

TIP

echo "== 5/5 معاينة سريعة =="
( SMART_DB="$SMART_DB" SMART_BIND=0.0.0.0:3300 nohup ./target/release/smart-accountant \
    > data/run.log 2>&1 & echo $! > data/run.pid )
sleep 2
if curl -fsS "http://127.0.0.1:3300/health" >/dev/null 2>&1 || curl -fsS "http://127.0.0.1:3300/" >/dev/null 2>&1; then
  echo "الخدمة تعمل ✔  (السجل: data/run.log — الإيقاف: kill \$(cat data/run.pid))"
else
  echo "لم تستجب الخدمة بعد؛ راجع data/run.log"
fi
