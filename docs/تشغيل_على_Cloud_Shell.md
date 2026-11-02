# تشغيل المشروع على Google Cloud Shell (خطوات جاهزة للنسخ)

> **تنبيه صريح:** لا أستطيع أنا الدخول إلى Cloud Shell الخاص بك ولا فتح نفق إليه؛
> جلستك هناك محمية بحسابك ولا أملك صلاحية الوصول إليها. ما أجهّزه لك: أوامر
> جاهزة للنسخ، وسكربت `cloudshell-setup.sh` داخل المشروع يقوم بكل خطوة تقنيًا
> بمجرد تشغيله عندك. تقنية النفق تحتاج رمزًا من جهازك، وهذا موضّح أدناه.

## 1) في Cloud Shell — سطران فقط
```bash
curl -L -o r44.zip "https://drive.google.com/uc?id=1YkoYyMxaHagDOYiuQuwwK6kLutvBzCTu&export=download"
unzip -q r44.zip -d ~/app && cd ~/app/smart-accountant && bash cloudshell-setup.sh
```
السكربت يثبّت Rust إن لم يكن موجودًا، يبني نسخة الإنتاج، يهيّئ قاعدة البيانات
والحساب «خالد / 1»، ويشغّل الخدمة على المنفذ 3300.

## 2) فتح الواجهة
من قائمة Cloud Shell: **Web Preview → Change port → 3300 → Preview**.
الدخول: `خالد` / `1` — **غيّر كلمة المرور فورًا**.

## 3) الإبقاء على التشغيل بعد إغلاق التبويب
Cloud Shell يُوقف الجلسة بعد خمول، فاستخدم أحد الخيارين:

**أ) إعادة تشغيل سريعة عند الحاجة**
```bash
cd ~/app/smart-accountant && bash cloudshell-setup.sh
```

**ب) تشغيل في الخلفية دائمًا ما دامت الجلسة مفتوحة**
```bash
cd ~/app/smart-accountant
nohup env SMART_DB="$PWD/data/smart.sqlite" SMART_BIND=0.0.0.0:3300 \
  ./target/release/smart-accountant > data/run.log 2>&1 &
echo $! > data/run.pid
```

**ج) نسخ احتياطي ونقل قاعدة البيانات**
```bash
# نسخة محلية
cp ~/app/smart-accountant/data/smart.sqlite ~/smart-$(date +%F).sqlite
# نقل إلى Drive (يحتاج صلاحية حسابك، وهو أمر تسمح به Cloud Shell)
gcloud storage cp ~/smart-$(date +%F).sqlite gs://<اسم-الحزمة>/ 2>/dev/null || \
  echo "استخدم Drive من المتصفح بدلًا من gsutil"
```

## 4) ما لا يمكنني فعله من هنا (بصراحة)
| المطلوب | الحالة |
|---|---|
| الدخول إلى Cloud Shell الخاص بك | غير ممكن: لا أملك جلسة حسابك |
| فتح نفق دائم من جهازك إلى هذه الجلسة | غير ممكن بلا رمز/توكين تمنحه لتطبيق نفق |
| تشغيل `cloudflared` في Cloud Shell لمشاركة رابط عام | ممكن منك بأمر واحد أدناه |
| تشغيل المشروع هنا ومراجعته | ✅ متاح الآن على المنفذ 3300 في هذه المساحة |

### نفق عام مجاني (تشغّله أنت في Cloud Shell)
```bash
# رابط عام مؤقت لنسخة Cloud Shell (يفيد للعرض على العميل)
curl -L -o cloudflared https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64
chmod +x cloudflared && ./cloudflared tunnel --url http://127.0.0.1:3300
```
يعطيك رابطًا مثل `https://xxxx.trycloudflare.com` — مجاني، ويبقى ما دام الأمر يعمل.

## 5) أوامر صيانة سريعة
```bash
cd ~/app/smart-accountant
cargo test --release          # 97 اختبارًا
python3 quality/gates.py      # كل بوابات الجودة
./target/release/smart-accountant --seed-more   # بيانات إضافية (أطراف/شيكات/أصول/رواتب)
kill $(cat data/run.pid)      # إيقاف الخدمة
```
