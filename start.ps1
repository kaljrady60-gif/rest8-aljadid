# start.ps1 — تشغيل المحاسب الذكي على ويندوز (نقرة واحدة).
# الطريقة: زر يمين على الملف → Run with PowerShell
# أو:      powershell -ExecutionPolicy Bypass -File .\start.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $root

$exe = Join-Path $root "target\release\smart-accountant.exe"
if (-not (Test-Path $exe)) {
  Write-Host "== 1/4 بناء المشروع (المرة الأولى فقط، 3-8 دقائق) ==" -ForegroundColor Cyan
  cargo build --release
  if ($LASTEXITCODE -ne 0) { Write-Host "فشل البناء — انسخ آخر 15 سطرًا وأرسلها." -ForegroundColor Red; Read-Host "Enter"; exit 1 }
}

$dbDir = Join-Path $root "data"
$db    = Join-Path $dbDir "smart.sqlite"
New-Item -ItemType Directory -Force $dbDir | Out-Null
$env:SMART_DB = $db
$env:SMART_BIND = "127.0.0.1:3300"
$env:SMART_EXPERIMENTAL_POSTING = "1"

if (-not (Test-Path $db)) {
  Write-Host "== 2/4 إنشاء قاعدة البيانات والحساب ==" -ForegroundColor Cyan
  $env:SMART_DEV_WEAK_PASSWORD = "1"
  Write-Host "أدخل: اسم المدير (خالد) ثم كلمة المرور (1) ثم تأكيدها"
  & $exe --init-admin
  & $exe --init-company "شركتي"
  & $exe --seed-demo
  Remove-Item Env:SMART_DEV_WEAK_PASSWORD -ErrorAction SilentlyContinue
} else {
  Write-Host "== 2/4 القاعدة موجودة — لا تغيير ==" -ForegroundColor DarkGray
}

Write-Host "== 3/4 فتح المتصفح على http://127.0.0.1:3300 ==" -ForegroundColor Cyan
Start-Process "http://127.0.0.1:3300"
Start-Sleep -Seconds 1
Write-Host "== 4/4 الخادم يعمل — للإيقاف Ctrl+C ==" -ForegroundColor Green
& $exe
