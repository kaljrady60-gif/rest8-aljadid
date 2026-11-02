//! قراءة الفواتير المصوّرة بالذكاء الاصطناعي — المزوّد يختاره المستخدم.
//!
//! ثلاثة أنماط، وكلها مجانية:
//! * **محلي**: Ollama على الجهاز (`SMART_AI_OLLAMA`، افتراضيًا `http://127.0.0.1:11434`)،
//!   والنموذج يُختار من قائمة النماذج المثبَّتة فعلًا (`/api/tags`).
//! * **سحابي**: Google Gemini بالطبقة المجانية، والنماذج تُجلب فعليًا (`/v1beta/models`)
//!   ولا يُعرض نموذج غير موجود.
//! * **هجين**: ترتيب يحدده المستخدم؛ يُجرَّب الأول فإن فشل يُسجَّل السبب ويُجرَّب التالي.
//!   **الرجوع آمن**: لا يُحفظ أي ناتج جزئي، ويُعاد للمستخدم سبب كل مزوّد فشل صراحةً.
//!
//! النموذج لا يُصدَّق أعمى: نصّه يمرّ على نفس المحلّل الحتمي، وما لا يُقرأ يُعلَن فشله.
//! ولا تُحفظ الصورة على القرص إطلاقًا.

use std::time::Duration;

const PROMPT: &str = "انسخ نص هذه الفاتورة كما هو، سطرًا بسطر، مع التاريخ وأسماء البنود \
والمبالغ والعملة والضريبة والإجمالي. لا تشرح ولا تُضف أي كلمة من عندك ولا تُترجم.";

/// نمط التشغيل المختار.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Local,
    Cloud,
    Hybrid,
}

impl Mode {
    pub fn parse(raw: &str) -> Mode {
        match raw {
            "cloud" => Mode::Cloud,
            "hybrid" => Mode::Hybrid,
            _ => Mode::Local,
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Mode::Local => "local",
            Mode::Cloud => "cloud",
            Mode::Hybrid => "hybrid",
        }
    }
}

/// إعدادات المستخدم كما تُحفظ في قاعدة البيانات.
#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    /// ترتيب المحاولة في النمط الهجين، عناصرها "local" أو "cloud".
    pub order: Vec<String>,
    pub local_endpoint: String,
    pub local_model: String,
    pub cloud_provider: String,
    pub cloud_model: String,
    pub cloud_key: String,
}

impl Default for Config {
    fn default() -> Self {
        let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let local_model = env("SMART_AI_MODEL").unwrap_or_else(|| "llava".into());
        let cloud_model = env("SMART_AI_GEMINI_MODEL").unwrap_or_else(|| "gemini-2.5-flash".into());
        let mode = if env("SMART_AI_GEMINI_KEY").is_some() && env("SMART_AI_OLLAMA").is_none() {
            Mode::Cloud
        } else if env("SMART_AI_OLLAMA").is_some() {
            Mode::Local
        } else {
            Mode::Hybrid
        };
        Config {
            mode,
            order: vec!["local".into(), "cloud".into()],
            local_endpoint: env("SMART_AI_OLLAMA")
                .unwrap_or_else(|| "http://127.0.0.1:11434".into())
                .trim_end_matches('/')
                .to_string(),
            local_model,
            cloud_provider: env("SMART_AI_CLOUD").unwrap_or_else(|| "gemini".into()),
            cloud_model,
            cloud_key: env("SMART_AI_GEMINI_KEY").unwrap_or_default(),
        }
    }
}

/// ما عاد من النموذج: النص الكامل + الحقول المؤكدة إن أعادها.
pub struct Reading {
    pub text: String,
    pub date: Option<String>,
    pub total: Option<String>,
    pub currency: Option<String>,
    pub provider: &'static str,
    pub model: String,
    /// سجل المحاولات في النمط الهجين: (المزوّد، نجح؟، السبب عند الفشل).
    pub attempts: Vec<(String, bool, Option<String>)>,
}

#[derive(Clone)]
enum Target {
    Local { endpoint: String, model: String },
    Cloud { key: String, model: String },
}

impl Target {
    fn label(&self) -> &'static str {
        match self {
            Target::Local { .. } => "local",
            Target::Cloud { .. } => "cloud",
        }
    }
    fn model(&self) -> String {
        match self {
            Target::Local { model, .. } | Target::Cloud { model, .. } => model.clone(),
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(90)))
        .build()
        .into()
}

fn parse_fields(raw: &str) -> (Option<String>, Option<String>, Option<String>) {
    (
        super::requests::find_date_str(raw),
        super::requests::find_total_str(raw),
        super::requests::find_currency_str(raw),
    )
}

// ─────────────────────────── جلب النماذج الفعلية ───────────────────────────
fn local_models_blocking(endpoint: &str) -> Result<Vec<String>, String> {
    let url = format!("{endpoint}/api/tags");
    let response = agent()
        .get(&url)
        .call()
        .map_err(|e| format!("تعذّر الوصول إلى Ollama على {endpoint}: {e}"))?;
    let value: serde_json::Value = response
        .into_body()
        .read_json()
        .map_err(|e| format!("ردّ Ollama غير مفهوم: {e}"))?;
    let models = value["models"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|m| m["name"].as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(models)
}

fn cloud_models_blocking(key: &str) -> Result<Vec<String>, String> {
    if key.trim().is_empty() {
        return Err("لا يوجد مفتاح سحابي محفوظ؛ أضف مفتاح Gemini المجاني أولًا.".into());
    }
    let url = "https://generativelanguage.googleapis.com/v1beta/models";
    let response = agent()
        .get(url)
        .header("x-goog-api-key", key)
        .call()
        .map_err(|e| format!("تعذّر الوصول إلى Gemini: {e}"))?;
    let value: serde_json::Value = response
        .into_body()
        .read_json()
        .map_err(|e| format!("ردّ Gemini غير مفهوم: {e}"))?;
    let models = value["models"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|m| {
                    m["supportedGenerationMethods"]
                        .as_array()
                        .map(|ways| ways.iter().any(|w| w.as_str() == Some("generateContent")))
                        .unwrap_or(false)
                })
                .filter_map(|m| {
                    m["name"]
                        .as_str()
                        .map(|n| n.trim_start_matches("models/").to_string())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if models.is_empty() {
        return Err("لم يُعد المزوّد أي نموذج يدعم قراءة الصور.".into());
    }
    Ok(models)
}

/// قائمة النماذج الحقيقية من المصدر نفسه — لا قائمة مخترعة.
pub async fn models(scope: &str, config: Config, provider: Option<String>) -> Result<Vec<String>, String> {
    let scope = scope.to_string();
    tokio::task::spawn_blocking(move || match scope.as_str() {
        "cloud" => {
            let _ = provider;
            cloud_models_blocking(&config.cloud_key)
        }
        _ => local_models_blocking(&config.local_endpoint),
    })
    .await
    .map_err(|e| format!("انقطع جلب النماذج: {e}"))?
}

/// اختبار سريع لمزوّد واحد (بلا صورة) — يبيّن إن كان الوصول ممكنًا فعلًا.
pub async fn probe(scope: &str, config: Config) -> Result<String, String> {
    let scope = scope.to_string();
    tokio::task::spawn_blocking(move || match scope.as_str() {
        "cloud" => cloud_models_blocking(&config.cloud_key).map(|m| {
            format!(
                "السحابة متاحة · {} نموذجًا · النموذج المختار: {}",
                m.len(),
                config.cloud_model
            )
        }),
        _ => local_models_blocking(&config.local_endpoint).map(|m| {
            format!(
                "المحلي متاح · {} نموذجًا مثبَّتًا · النموذج المختار: {}",
                m.len(),
                config.local_model
            )
        }),
    })
    .await
    .map_err(|e| format!("انقطع الاختبار: {e}"))?
}

// ─────────────────────────────── القراءة ───────────────────────────────
fn read_local(endpoint: &str, model: &str, image: &str) -> Result<String, String> {
    let url = format!("{endpoint}/api/generate");
    let body = serde_json::json!({
        "model": model,
        "prompt": PROMPT,
        "images": [image],
        "stream": false,
        "options": { "temperature": 0 }
    });
    let response = agent()
        .post(&url)
        .header("Content-Type", "application/json")
        .send_json(&body)
        .map_err(|e| format!("تعذّر الوصول إلى Ollama على {endpoint}: {e}"))?;
    let value: serde_json::Value = response
        .into_body()
        .read_json()
        .map_err(|e| format!("ردّ Ollama غير مفهوم: {e}"))?;
    value["response"]
        .as_str()
        .filter(|t| !t.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| "لم يُعد النموذج المحلي أي نص للصورة.".to_string())
}

fn read_cloud(key: &str, model: &str, image: &str, mime: &str) -> Result<String, String> {
    if key.trim().is_empty() {
        return Err("لا يوجد مفتاح سحابي محفوظ.".into());
    }
    let url =
        format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent");
    let body = serde_json::json!({
        "contents": [{
            "parts": [
                { "text": PROMPT },
                { "inline_data": { "mime_type": mime, "data": image } }
            ]
        }],
        "generationConfig": { "temperature": 0 }
    });
    let response = agent()
        .post(&url)
        .header("Content-Type", "application/json")
        .header("x-goog-api-key", key)
        .send_json(&body)
        .map_err(|e| format!("تعذّر الوصول إلى Gemini: {e}"))?;
    let value: serde_json::Value = response
        .into_body()
        .read_json()
        .map_err(|e| format!("ردّ Gemini غير مفهوم: {e}"))?;
    value["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .filter(|t| !t.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| "لم يُعد Gemini أي نص (قد تكون الصورة غير واضحة).".to_string())
}

/// يقرأ الصورة وفق اختيار المستخدم. في الهجين: يجرّب بالترتيب، ويعلن سبب كل فشل.
pub async fn read_invoice(config: Config, image: String, mime: String) -> Result<Reading, String> {
    let order: Vec<String> = match config.mode {
        Mode::Local => vec!["local".into()],
        Mode::Cloud => vec!["cloud".into()],
        Mode::Hybrid => {
            if config.order.is_empty() {
                vec!["local".into(), "cloud".into()]
            } else {
                config.order.clone()
            }
        }
    };
    let targets: Vec<Target> = order
        .iter()
        .map(|slot| match slot.as_str() {
            "cloud" => Target::Cloud {
                key: config.cloud_key.clone(),
                model: config.cloud_model.clone(),
            },
            _ => Target::Local {
                endpoint: config.local_endpoint.clone(),
                model: config.local_model.clone(),
            },
        })
        .collect();

    tokio::task::spawn_blocking(move || {
        let mut attempts: Vec<(String, bool, Option<String>)> = Vec::new();
        for target in targets {
            let model = target.model();
            let outcome = match &target {
                Target::Local { endpoint, model } => read_local(endpoint, model, &image),
                Target::Cloud { key, model } => read_cloud(key, model, &image, &mime),
            };
            match outcome {
                Ok(text) => {
                    let (date, total, currency) = parse_fields(&text);
                    attempts.push((target.label().to_string(), true, None));
                    return Ok(Reading {
                        text,
                        date,
                        total,
                        currency,
                        provider: target.label(),
                        model,
                        attempts,
                    });
                }
                Err(reason) => attempts.push((target.label().to_string(), false, Some(reason))),
            }
        }
        // لا يوجد ناتج جزئي يُحفظ: نُعيد سبب كل محاولة ونترك المستخدم يقرّر.
        let detail = attempts
            .iter()
            .map(|(scope, _, reason)| {
                format!(
                    "{}: {}",
                    if scope == "local" { "المحلي" } else { "السحابي" },
                    reason.clone().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join(" · ");
        Err(format!("فشلت كل المحاولات — {detail}"))
    })
    .await
    .map_err(|e| format!("انقطع تنفيذ القراءة: {e}"))?
}

/// حالة المزوّدين بلا شبكة: ما هو مضبوط وما هو ناقص، بالعربية وبصراحة.
pub fn status(config: &Config, saved: bool) -> serde_json::Value {
    let local_ok = !config.local_endpoint.trim().is_empty();
    let cloud_ok = !config.cloud_key.trim().is_empty();
    let ready = match config.mode {
        Mode::Local => local_ok,
        Mode::Cloud => cloud_ok,
        Mode::Hybrid => local_ok || cloud_ok,
    };
    let note = if ready {
        format!(
            "النمط: {} — سيُقرأ بـ{}.",
            match config.mode {
                Mode::Local => "محلي فقط",
                Mode::Cloud => "سحابي فقط",
                Mode::Hybrid => "هجين (محلي ثم سحابي بحسب ترتيبك)",
            },
            match config.mode {
                Mode::Local => format!(" نموذج محلي: {}", config.local_model),
                Mode::Cloud => format!(" Gemini: {}", config.cloud_model),
                Mode::Hybrid => format!(
                    " أول مزوّد متاح بالترتيب ({})",
                    config.order.join(" → ")
                ),
            }
        )
    } else if config.mode == Mode::Cloud {
        "النمط السحابي يحتاج مفتاح Gemini المجاني. أضفه ثم أعد المحاولة.".to_string()
    } else {
        "لم يُضبط مزوّد. شغّل Ollama محليًا (ollama serve ثم ollama pull llava) \
         أو أضف مفتاح Gemini المجاني."
            .to_string()
    };
    serde_json::json!({
        "ready": ready,
        "saved": saved,
        "mode": config.mode.code(),
        "order": config.order,
        "local": {
            "endpoint": config.local_endpoint,
            "model": config.local_model,
            "configured": local_ok
        },
        "cloud": {
            "provider": config.cloud_provider,
            "model": config.cloud_model,
            "configured": cloud_ok
        },
        "note": note
    })
}
