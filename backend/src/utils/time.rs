use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc};
use chrono_tz::Asia::Shanghai;

pub fn pad2(n: u32) -> String {
    format!("{:02}", n)
}

pub fn shanghai_now_iso() -> String {
    let now = Utc::now().with_timezone(&Shanghai);
    now.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

pub fn shanghai_now_epoch_ms() -> i64 {
    Utc::now().timestamp_millis()
}

pub fn shanghai_now_date_key() -> String {
    let now = Utc::now().with_timezone(&Shanghai);
    format!("{}-{}-{}", now.year(), pad2(now.month()), pad2(now.day()))
}

pub fn get_shanghai_date_time_parts(now: DateTime<Utc>) -> (i32, u32, u32, u32, u32, u32) {
    let sh = now.with_timezone(&Shanghai);
    (
        sh.year(),
        sh.month(),
        sh.day(),
        sh.hour(),
        sh.minute(),
        sh.second(),
    )
}

pub fn format_epoch_ms_to_shanghai_iso(epoch_ms: i64) -> String {
    match DateTime::from_timestamp_millis(epoch_ms) {
        Some(dt) => {
            let sh = dt.with_timezone(&Shanghai);
            sh.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
        }
        None => String::new(),
    }
}

pub fn parse_date_time_to_epoch_ms(value: &str) -> Option<i64> {
    if value.is_empty() {
        return None;
    }
    // Try ISO format
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Some(dt.timestamp_millis());
    }
    // Try YYYY-MM-DD
    if let Ok(d) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        let dt = d.and_hms_opt(0, 0, 0)?;
        let sh_dt = Shanghai.from_local_datetime(&dt).single()?;
        return Some(sh_dt.timestamp_millis());
    }
    None
}

pub fn add_days_to_date_key(date_key: &str, days: i64) -> String {
    if let Ok(d) = NaiveDate::parse_from_str(date_key, "%Y-%m-%d") {
        let new_d = d + Duration::days(days);
        return new_d.format("%Y-%m-%d").to_string();
    }
    date_key.to_string()
}

pub fn get_date_diff_in_days(start: &str, end: &str) -> i64 {
    let s = NaiveDate::parse_from_str(start, "%Y-%m-%d");
    let e = NaiveDate::parse_from_str(end, "%Y-%m-%d");
    match (s, e) {
        (Ok(s), Ok(e)) => (e - s).num_days(),
        _ => 0,
    }
}

pub fn is_weekend(date_key: &str) -> bool {
    if let Ok(d) = NaiveDate::parse_from_str(date_key, "%Y-%m-%d") {
        let dow = d.format("%u").to_string().parse::<u32>().unwrap_or(0);
        return dow == 5 || dow == 6 || dow == 7; // Fri/Sat/Sun
    }
    false
}

pub fn normalize_business_date(value: &serde_json::Value) -> Result<String, String> {
    match value {
        serde_json::Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return Err("日期为空".to_string());
            }
            // Normalize full-width characters to ASCII
            let normalized = s
                .replace(['\u{FF0E}', '\u{3002}'], ".") // ．/。→ .
                .replace('\u{FF0F}', "/") // ／→ /
                .replace(['\u{FF0D}', '\u{5E74}', '\u{6708}'], "-") // －/年/月 → -
                .replace('\u{65E5}', "") // 日 → (remove)
                .trim()
                .to_string();
            // Try normalized string first, fall back to original
            if let Ok(d) = NaiveDate::parse_from_str(&normalized, "%Y-%m-%d") {
                return Ok(d.format("%Y-%m-%d").to_string());
            }
            if let Ok(d) = NaiveDate::parse_from_str(&normalized, "%Y/%m/%d") {
                return Ok(d.format("%Y-%m-%d").to_string());
            }
            if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                return Ok(d.format("%Y-%m-%d").to_string());
            }
            if let Ok(d) = NaiveDate::parse_from_str(s, "%Y/%m/%d") {
                return Ok(d.format("%Y-%m-%d").to_string());
            }
            Err(format!("无效日期: {}", s))
        }
        serde_json::Value::Number(n) => {
            // Excel serial date number
            if let Some(f) = n.as_f64()
                && (1.0..=2958465.0).contains(&f)
            {
                let excel_epoch = NaiveDate::from_ymd_opt(1899, 12, 30).unwrap();
                let days = f.floor() as i64;
                let d = excel_epoch + Duration::days(days);
                return Ok(d.format("%Y-%m-%d").to_string());
            }
            Err(format!("无效Excel日期数字: {}", n))
        }
        serde_json::Value::Object(obj) => {
            if let Some(v) = obj
                .get("v")
                .or_else(|| obj.get("w"))
                .or_else(|| obj.get("text"))
            {
                return normalize_business_date(v);
            }
            Err("日期对象缺少 v/w/text 字段".to_string())
        }
        _ => Err("日期格式不支持".to_string()),
    }
}

pub fn format_export_timestamp() -> String {
    let now = Utc::now().with_timezone(&Shanghai);
    format!(
        "{}{:02}{:02}{:02}{:02}{:02}",
        now.year(),
        now.month(),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}
