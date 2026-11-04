//! Opt-in Claude API client (raw HTTPS; Rust has no official Anthropic SDK).
//!
//! * Off until the user enables it and acknowledges what is sent and what it costs.
//! * The API key lives in the macOS Keychain (`secrets`), never in SQLite.
//! * Only job-posting text and the verified-profile facts needed for the document are sent —
//!   never contact details, notes, application history or other jobs.
//! * Every call is recorded in `ai_usage` with tokens and estimated cost.

use crate::db;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const API_VERSION: &str = "2026-10-21";
/// `fallbacks: "default"` re-runs a declined request on Anthropic's recommended model.
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
pub const MODELS: [&str; 3] = ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"];

pub const DEFAULT_STYLE: &str = "Professional, warm and concise. Plain words over jargon. Concrete specifics over adjectives. Confident without overselling.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    pub enabled: bool,
    /// When the user acknowledged the data-sharing and cost disclosure.
    pub acknowledged_at: Option<String>,
    pub model: String,
    pub effort: String,
    /// Notes on the applicant's voice, used for cover letters. Editable by the user.
    pub style_notes: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings { enabled: false, acknowledged_at: None, model: DEFAULT_MODEL.into(), effort: "high".into(), style_notes: DEFAULT_STYLE.into() }
    }
}

pub fn get_settings(c: &Connection) -> AppResult<AiSettings> {
    Ok(db::get_setting(c, "ai_settings")?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default())
}

pub fn save_settings(c: &Connection, s: &AiSettings) -> AppResult<()> {
    if !MODELS.contains(&s.model.as_str()) {
        return Err(AppError::invalid("Choose Claude Opus 5.5, Sonnet 5.5 or Haiku 5.5"));
    }
    if !["low", "medium", "high", "xhigh"].contains(&s.effort.as_str()) {
        return Err(AppError::invalid("Effort must be low, medium, high or xhigh"));
    }
    if s.enabled && s.acknowledged_at.is_none() {
        return Err(AppError::invalid("Review and accept the data and cost disclosure before turning AI writing on"));
    }
    db::set_setting(c, "ai_settings", &serde_json::to_string(s).map_err(AppError::other)?)?;
    db::log(c, "info", "ai", &format!("AI writing {} ({} · effort {})", if s.enabled { "on" } else { "off" }, s.model, s.effort));
    Ok(())
}

/// (input, output, cache read) US$ per million tokens — first-party API list prices.
pub fn pricing(model: &str) -> (f64, f64, f64) {
    match model {
        "claude-sonnet-5-5" => (2.0, 10.0, 0.20),
        "claude-haiku-5-5" => (0.10, 0.50, 0.01),
        _ => (4.0, 20.0, 0.20),
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
}

pub fn cost_usd(model: &str, u: &Usage) -> f64 {
    let (i, o, cr) = pricing(model);
    (u.input_tokens as f64 * i + u.cache_write_tokens as f64 * i * 1.25 + u.cache_read_tokens as f64 * cr + u.output_tokens as f64 * o) / 1_000_000.0
}

/// Rough pre-flight estimate shown before a call (4 characters ≈ 1 token; output incl. thinking).
pub fn estimate_usd(model: &str, prompt_chars: usize, expected_output_tokens: i64) -> f64 {
    cost_usd(model, &Usage { input_tokens: (prompt_chars / 4) as i64, output_tokens: expected_output_tokens, ..Default::default() })
}

pub fn build_request(model: &str, effort: &str, system: &str, user: &str, schema: &Value) -> Value {
    let mut body = json!({
        "model": model,
        "max_tokens": 16000,
        "output_config": { "effort": effort, "format": { "type": "json_schema", "schema": schema } },
        "system": [{ "type": "text", "text": system, "cache_control": { "type": "ephemeral" } }],
        "messages": [{ "role": "user", "content": user }],
    });
    if uses_fallbacks(model) {
        body["fallbacks"] = json!("default");
    }
    body
}

fn uses_fallbacks(model: &str) -> bool {
    model == "claude-opus-5-5" || model == "claude-sonnet-5-5"
}

#[derive(Debug, Clone)]
pub struct AiResult {
    pub json: Value,
    pub model: String,
    pub usage: Usage,
    pub served_by_fallback: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AiFailure {
    Refused(String),
    Truncated,
    BadOutput(String),
}

impl std::fmt::Display for AiFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiFailure::Refused(why) => write!(f, "Claude declined this request ({why}). Nothing was saved."),
            AiFailure::Truncated => write!(f, "The response was cut off before it finished. Try again, or lower the effort setting."),
            AiFailure::BadOutput(e) => write!(f, "Claude's response could not be read ({e}). Try again."),
        }
    }
}

pub fn usage_of(v: &Value) -> Usage {
    let u = v.get("usage");
    let n = |k: &str| u.and_then(|x| x.get(k)).and_then(Value::as_i64).unwrap_or(0);
    Usage {
        input_tokens: n("input_tokens"),
        output_tokens: n("output_tokens"),
        cache_read_tokens: n("cache_read_input_tokens"),
        cache_write_tokens: n("cache_creation_input_tokens"),
    }
}

/// Checks the stop reason before reading content, then parses the structured JSON output.
pub fn parse_response(v: &Value) -> Result<AiResult, AiFailure> {
    let stop = v.get("stop_reason").and_then(Value::as_str).unwrap_or("");
    if stop == "refusal" {
        let d = v.get("stop_details");
        let why = d
            .and_then(|d| d.get("explanation").and_then(Value::as_str).or_else(|| d.get("category").and_then(Value::as_str)))
            .unwrap_or("no reason given");
        return Err(AiFailure::Refused(why.to_string()));
    }
    if stop == "max_tokens" {
        return Err(AiFailure::Truncated);
    }
    let text: String = v
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect();
    let json: Value = serde_json::from_str(text.trim()).map_err(|e| AiFailure::BadOutput(e.to_string()))?;
    let served_by_fallback = v
        .get("usage")
        .and_then(|u| u.get("iterations"))
        .and_then(Value::as_array)
        .map(|a| a.iter().any(|i| i.get("type").and_then(Value::as_str) == Some("fallback_message")))
        .unwrap_or(false);
    Ok(AiResult { json, model: v.get("model").and_then(Value::as_str).unwrap_or("").to_string(), usage: usage_of(v), served_by_fallback })
}

/// Sends one request. Retries transient failures (429, 5xx, 529 overloaded, network) twice.
pub async fn send(api_key: &str, body: &Value) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;
    let model = body.get("model").and_then(Value::as_str).unwrap_or("");
    let mut last = String::new();
    for attempt in 0..3u64 {
        if attempt > 0 {
            let wait = 2000 * attempt;
            tauri::async_runtime::spawn_blocking(move || std::thread::sleep(std::time::Duration::from_millis(wait))).await.ok();
        }
        let mut req = client
            .post(API_URL)
            .header("x-api-key", api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(body);
        if uses_fallbacks(model) {
            req = req.header("anthropic-beta", FALLBACK_BETA);
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                last = format!("Network error contacting Anthropic: {e}");
                continue;
            }
        };
        let status = resp.status().as_u16();
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        let api_msg = v.get("error").and_then(|e| e.get("message")).and_then(Value::as_str).unwrap_or("").to_string();
        match status {
            200 => return Ok(v),
            401 | 403 => return Err("Anthropic rejected the API key. Replace it in Preferences → AI writing.".into()),
            400 if api_msg.to_lowercase().contains("credit balance") => {
                return Err("Your Anthropic account has no API credits available. API credits are bought at console.anthropic.com → Billing, in the same organization as your API key (a claude.ai subscription does not include API credits). New credits can take a few minutes to apply. Nothing was charged.".into())
            }
            400 | 404 | 413 => return Err(format!("Anthropic could not process the request (HTTP {status}): {api_msg}")),
            429 | 500..=599 => last = format!("Anthropic is busy or rate-limited (HTTP {status}). {api_msg}"),
            _ => return Err(format!("Unexpected response from Anthropic (HTTP {status}): {api_msg}")),
        }
    }
    Err(last)
}

pub fn record_usage(c: &Connection, purpose: &str, job_id: Option<i64>, model: &str, u: &Usage, outcome: &str) -> AppResult<f64> {
    let cost = cost_usd(model, u);
    c.execute(
        "INSERT INTO ai_usage (ts, purpose, job_id, model, input_tokens, output_tokens, cache_read_tokens, cost_usd, outcome)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![db::now(), purpose, job_id, model, u.input_tokens + u.cache_write_tokens, u.output_tokens, u.cache_read_tokens, cost, outcome],
    )?;
    db::log(
        c,
        if outcome == "ok" { "info" } else { "warn" },
        "ai",
        &format!("{purpose} via {model}: {} in / {} out tokens, ≈${cost:.3} ({outcome})", u.input_tokens + u.cache_write_tokens + u.cache_read_tokens, u.output_tokens),
    );
    Ok(cost)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub calls: i64,
    pub total_usd: f64,
    pub month_usd: f64,
}

pub fn usage_summary(c: &Connection) -> AppResult<UsageSummary> {
    let month = format!("{}%", &db::now()[..7]);
    let (calls, total): (i64, f64) = c.query_row("SELECT COUNT(*), COALESCE(SUM(cost_usd),0) FROM ai_usage", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let month_usd: f64 = c.query_row("SELECT COALESCE(SUM(cost_usd),0) FROM ai_usage WHERE ts LIKE ?1", [month], |r| r.get(0))?;
    Ok(UsageSummary { calls, total_usd: total, month_usd })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_shape() {
        let schema = json!({"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"], "additionalProperties": false});
        let b = build_request("claude-opus-5-5", "high", "SYS", "USER", &schema);
        assert_eq!(b["model"], "claude-opus-5-5");
        assert_eq!(b["output_config"]["effort"], "high");
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
        assert_eq!(b["fallbacks"], "default");
        assert!(b.get("thinking").is_none(), "Opus 5.5 thinks adaptively; never send a disabled/budget config");
        assert!(b.get("temperature").is_none());
        assert_eq!(b["system"][0]["cache_control"]["type"], "ephemeral");
        let h = build_request("claude-haiku-5-5", "medium", "S", "U", &schema);
        assert!(h.get("fallbacks").is_none(), "Haiku has no server-side fallback");
    }

    #[test]
    fn parses_ok_refusal_and_truncation() {
        let ok = json!({"model": "claude-opus-5-5", "stop_reason": "end_turn",
            "content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": "{\"a\": \"b\"}"}],
            "usage": {"input_tokens": 1000, "output_tokens": 2000, "cache_read_input_tokens": 500, "iterations": [{"type": "message"}]}});
        let r = parse_response(&ok).unwrap();
        assert_eq!(r.json["a"], "b");
        assert!(!r.served_by_fallback);
        assert_eq!(r.usage.cache_read_tokens, 500);

        let refused = json!({"stop_reason": "refusal", "stop_details": {"type": "refusal", "category": "cyber", "explanation": null}, "content": []});
        assert_eq!(parse_response(&refused).unwrap_err(), AiFailure::Refused("cyber".into()));
        assert_eq!(parse_response(&json!({"stop_reason": "max_tokens", "content": []})).unwrap_err(), AiFailure::Truncated);
        assert!(matches!(parse_response(&json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": "not json"}]})), Err(AiFailure::BadOutput(_))));

        let fb = json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": "{}"}], "usage": {"iterations": [{"type": "message"}, {"type": "fallback_message"}]}});
        assert!(parse_response(&fb).unwrap().served_by_fallback);
    }

    #[test]
    fn costs() {
        let u = Usage { input_tokens: 10_000, output_tokens: 5_000, cache_read_tokens: 0, cache_write_tokens: 0 };
        assert!((cost_usd("claude-opus-5-5", &u) - 0.14).abs() < 1e-9);
        assert!((cost_usd("claude-sonnet-5-5", &u) - 0.07).abs() < 1e-9);
        assert!(estimate_usd("claude-opus-5-5", 40_000, 6000) > 0.1);
    }

    #[test]
    fn settings_require_acknowledgement() {
        let c = db::open_in_memory().unwrap();
        let s = get_settings(&c).unwrap();
        assert!(!s.enabled, "off by default");
        assert_eq!(s.model, "claude-opus-5-5");
        let mut on = s.clone();
        on.enabled = true;
        assert!(save_settings(&c, &on).is_err(), "cannot enable without acknowledging the disclosure");
        on.acknowledged_at = Some(db::now());
        save_settings(&c, &on).unwrap();
        assert!(get_settings(&c).unwrap().enabled);
        on.model = "gpt-5".into();
        assert!(save_settings(&c, &on).is_err());
        record_usage(&c, "resume", None, "claude-opus-5-5", &Usage { input_tokens: 10_000, output_tokens: 5_000, ..Default::default() }, "ok").unwrap();
        let sum = usage_summary(&c).unwrap();
        assert_eq!(sum.calls, 1);
        assert!((sum.total_usd - 0.14).abs() < 1e-9);
    }
}
