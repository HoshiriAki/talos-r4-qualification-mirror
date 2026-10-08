use std::env;
use std::net::IpAddr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatabaseProfile {
    SqliteLocal,
    Postgres18,
}

pub fn resolve_database_profile(
    raw_backend: Option<&str>,
    is_production: bool,
) -> Result<DatabaseProfile, &'static str> {
    let default_backend = if is_production { "postgres" } else { "sqlite" };
    let backend = raw_backend
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_backend);

    let profile = match backend.to_ascii_lowercase().as_str() {
        "sqlite" => DatabaseProfile::SqliteLocal,
        "postgres" | "postgresql" | "postgres18" => DatabaseProfile::Postgres18,
        _ => return Err("DB_BACKEND must be sqlite or postgres"),
    };

    if is_production && profile != DatabaseProfile::Postgres18 {
        return Err("production requires the PostgreSQL 18 database profile");
    }

    Ok(profile)
}

pub fn database_profile_from_env(is_production: bool) -> Result<DatabaseProfile, &'static str> {
    let raw_backend = env::var("DB_BACKEND").ok();
    resolve_database_profile(raw_backend.as_deref(), is_production)
}

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub db_path: String,
    pub is_production: bool,
    pub public_https: bool,
    pub auth_cookie_name: String,
    pub session_ttl_days: i64,
    pub auth_cookie_secure: bool,
    pub auth_bootstrap_on_start: bool,
    pub auth_bootstrap_admin_username: String,
    pub auth_bootstrap_admin_password: String,
    pub auth_login_rate_window_ms: i64,
    pub auth_login_rate_max_attempts: u32,
    pub auth_login_rate_block_ms: i64,
    pub cors_allowed_origin: Option<String>,
    pub public_dir: String,
    /// When set (e.g. http://localhost:5173), Rust skips serving static files
    /// and redirects non-API browser requests to the Vite dev server.
    pub vite_dev_url: Option<String>,
}

fn to_bool(value: &str, default: bool) -> bool {
    match value.to_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" | "" => false,
        _ => default,
    }
}

fn to_number<T: std::str::FromStr>(value: &str, default: T) -> T {
    value.parse().unwrap_or(default)
}

fn validate_vite_dev_url(raw: &str, is_production: bool) -> Option<String> {
    if is_production || raw.is_empty() {
        return None;
    }
    let parsed = url::Url::parse(raw).ok()?;
    if parsed.scheme() != "http" {
        return None;
    }
    let host = parsed.host_str()?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .ok()
            .is_some_and(|address| address.is_loopback());
    loopback.then(|| raw.trim_end_matches('/').to_string())
}

/// Validate the single credential-bearing browser origin accepted by CORS.
///
/// A CORS origin is an origin tuple, not an arbitrary header string or URL.
/// Normalize an optional trailing slash/default port while rejecting wildcard,
/// credentials, path/query/fragment material and non-HTTP(S) schemes. This also
/// prevents tower-http from receiving the invalid `* + credentials` pairing.
pub fn normalize_cors_origin(raw: &str, require_https: bool) -> Result<String, &'static str> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("origin is empty");
    }
    if raw == "*" {
        return Err("wildcard origin is forbidden with credentials");
    }

    let parsed = url::Url::parse(raw).map_err(|_| "origin is not a valid URL")?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("origin scheme must be http or https");
    }
    if require_https && parsed.scheme() != "https" {
        return Err("PUBLIC_HTTPS requires an https origin");
    }
    if parsed.host_str().is_none() {
        return Err("origin host is missing");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("origin must not contain userinfo");
    }
    if parsed.path() != "/" || parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("origin must not contain path, query or fragment");
    }

    let normalized = parsed.origin().ascii_serialization();
    if normalized == "null" {
        return Err("origin is opaque");
    }
    Ok(normalized)
}

impl AppConfig {
    pub fn from_env() -> Self {
        let is_production = env::var("NODE_ENV").unwrap_or_default() == "production";
        let public_https =
            is_production && to_bool(&env::var("PUBLIC_HTTPS").unwrap_or_default(), false);
        let public_dir = env::var("PUBLIC_DIR").unwrap_or_else(|_| "../public".to_string());
        let db_path = env::var("DB_PATH").unwrap_or_else(|_| "../rental.db".to_string());
        // Resolve relative paths from the project root
        let public_dir = if std::path::Path::new(&public_dir).is_absolute() {
            public_dir
        } else {
            std::path::Path::new(&std::env::current_dir().unwrap_or_default())
                .join(&public_dir)
                .to_string_lossy()
                .to_string()
        };
        let db_path = if std::path::Path::new(&db_path).is_absolute() {
            db_path
        } else {
            std::path::Path::new(&std::env::current_dir().unwrap_or_default())
                .join(&db_path)
                .to_string_lossy()
                .to_string()
        };

        // Production browser sessions may never downgrade the Secure cookie
        // attribute through environment configuration. Development retains the
        // explicit override so loopback HTTP remains usable.
        let auth_cookie_secure = if is_production {
            true
        } else {
            to_bool(&env::var("AUTH_COOKIE_SECURE").unwrap_or_default(), false)
        };
        let vite_dev_url = env::var("VITE_DEV_URL")
            .ok()
            .and_then(|value| validate_vite_dev_url(&value, is_production));

        Self {
            host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: to_number(&env::var("PORT").unwrap_or_default(), 3000u16),
            db_path,
            is_production,
            public_https,
            auth_cookie_name: env::var("AUTH_COOKIE_NAME")
                .unwrap_or_else(|_| "talos_session".to_string()),
            session_ttl_days: to_number(&env::var("SESSION_TTL_DAYS").unwrap_or_default(), 7i64),
            auth_cookie_secure,
            auth_bootstrap_on_start: to_bool(
                &env::var("AUTH_BOOTSTRAP_ON_START").unwrap_or_default(),
                false,
            ),
            auth_bootstrap_admin_username: env::var("AUTH_BOOTSTRAP_ADMIN_USERNAME")
                .unwrap_or_default(),
            auth_bootstrap_admin_password: env::var("AUTH_BOOTSTRAP_ADMIN_PASSWORD")
                .unwrap_or_default(),
            auth_login_rate_window_ms: to_number(
                &env::var("AUTH_LOGIN_RATE_WINDOW_MS").unwrap_or_default(),
                60_000i64,
            ),
            auth_login_rate_max_attempts: to_number(
                &env::var("AUTH_LOGIN_RATE_MAX_ATTEMPTS").unwrap_or_default(),
                5u32,
            ),
            auth_login_rate_block_ms: to_number(
                &env::var("AUTH_LOGIN_RATE_BLOCK_MS").unwrap_or_default(),
                300_000i64,
            ),
            cors_allowed_origin: env::var("CORS_ALLOWED_ORIGIN")
                .ok()
                .filter(|s| !s.is_empty()),
            public_dir,
            vite_dev_url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DatabaseProfile, normalize_cors_origin, resolve_database_profile, validate_vite_dev_url,
    };

    #[test]
    fn database_profile_defaults_are_environment_safe() {
        assert_eq!(
            resolve_database_profile(None, false).unwrap(),
            DatabaseProfile::SqliteLocal
        );
        assert_eq!(
            resolve_database_profile(None, true).unwrap(),
            DatabaseProfile::Postgres18
        );
    }

    #[test]
    fn production_database_profile_fails_closed() {
        assert_eq!(
            resolve_database_profile(Some("postgresql"), true).unwrap(),
            DatabaseProfile::Postgres18
        );
        assert!(resolve_database_profile(Some("sqlite"), true).is_err());
        assert!(resolve_database_profile(Some("mysql"), true).is_err());
    }

    #[test]
    fn vite_redirect_target_is_dev_loopback_only() {
        assert_eq!(
            validate_vite_dev_url("http://localhost:5173", false).as_deref(),
            Some("http://localhost:5173")
        );
        assert_eq!(
            validate_vite_dev_url("http://127.0.0.1:5173/", false).as_deref(),
            Some("http://127.0.0.1:5173")
        );
        assert!(validate_vite_dev_url("https://example.com", false).is_none());
        assert!(validate_vite_dev_url("http://example.com:5173", false).is_none());
        assert!(validate_vite_dev_url("http://localhost:5173", true).is_none());
    }

    #[test]
    fn cors_origin_is_single_http_origin_and_normalizes_defaults() {
        assert_eq!(
            normalize_cors_origin("https://example.test/", true).unwrap(),
            "https://example.test"
        );
        assert_eq!(
            normalize_cors_origin("https://example.test:443", true).unwrap(),
            "https://example.test"
        );
        assert_eq!(
            normalize_cors_origin("http://localhost:5173/", false).unwrap(),
            "http://localhost:5173"
        );
    }

    #[test]
    fn credential_cors_rejects_wildcard_and_non_origin_urls() {
        for invalid in [
            "*",
            "null",
            "file:///tmp/index.html",
            "https://user@example.test",
            "https://example.test/app",
            "https://example.test/?q=1",
            "https://example.test/#fragment",
        ] {
            assert!(normalize_cors_origin(invalid, false).is_err(), "{invalid}");
        }
        assert!(normalize_cors_origin("http://example.test", true).is_err());
    }
}
