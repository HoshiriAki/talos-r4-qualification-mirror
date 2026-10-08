use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use serde::Serialize;

pub fn json_response<T: Serialize>(status: StatusCode, body: &T) -> Response {
    let json = serde_json::to_string(body).unwrap_or_else(|_| "{}".to_string());
    let mut res = Response::new(axum::body::Body::from(json));
    *res.status_mut() = status;
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    res
}

pub fn binary_response(
    status: StatusCode,
    content_type: &str,
    file_name: &str,
    buffer: Vec<u8>,
) -> Response {
    let mut res = Response::new(axum::body::Body::from(buffer));
    *res.status_mut() = status;
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    res.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", file_name)).unwrap(),
    );
    res
}

pub fn build_set_cookie(
    name: &str,
    value: &str,
    max_age_secs: i64,
    secure: bool,
    http_only: bool,
) -> HeaderValue {
    // Percent-encode the value to match Node.js encodeURIComponent behavior.
    // While UUIDs are currently safe, this prevents malformed cookies if
    // the session format ever changes to include special characters.
    let encoded_value: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
                c.to_string()
            } else {
                format!("%{:02X}", c as u8)
            }
        })
        .collect();

    let mut cookie = format!(
        "{}={}; Path=/; SameSite=Lax; Max-Age={}",
        name, encoded_value, max_age_secs
    );
    if secure {
        cookie.push_str("; Secure");
    }
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    HeaderValue::from_str(&cookie).unwrap()
}

pub fn build_clear_cookie(name: &str, secure: bool, http_only: bool) -> HeaderValue {
    let mut cookie = format!("{}=; Path=/; SameSite=Lax; Max-Age=0", name);
    if secure {
        cookie.push_str("; Secure");
    }
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    HeaderValue::from_str(&cookie).unwrap()
}

pub fn parse_auth_cookie(cookie_header: &str, cookie_name: &str) -> Option<String> {
    for part in cookie_header.split(';') {
        let trimmed = part.trim();
        if let Some((key, value)) = trimmed.split_once('=')
            && key.trim() == cookie_name
        {
            return Some(value.trim().to_string());
        }
    }
    None
}
