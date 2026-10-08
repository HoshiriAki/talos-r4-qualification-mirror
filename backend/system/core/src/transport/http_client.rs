/// HTTP 客户端 trait — 所有出站 HTTP 调用的统一抽象。
///
/// 设计边界（trait 不该做什么）：
/// - 不做业务签名（MD5/RSA/…）→ 调用方职责
/// - 不解析响应体格式（XML/JSON/…）→ 调用方职责
/// - 不做业务级重试 → 调用方决定策略
/// - 不管 token 刷新 → 通过 Scheduler trait 管理
pub trait HttpClient: Send + Sync {
    /// 发送 HTTP 请求，返回原始响应。
    /// 不做业务级重试——由调用方决定。
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, String>;

    /// 连通性检查
    fn ping(&self, base_url: &str) -> Result<bool, String>;
}

#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// HTTP 方法：GET | POST | PUT | DELETE
    pub method: String,
    /// 完整 URL
    pub url: String,
    /// 请求头列表
    pub headers: Vec<(String, String)>,
    /// 原始请求体字节——XML/JSON/表单 由调用方决定
    pub body: Option<Vec<u8>>,
    /// 超时毫秒，默认 5000
    pub timeout_ms: u64,
}

impl Default for HttpRequest {
    fn default() -> Self {
        Self {
            method: "GET".into(),
            url: String::new(),
            headers: Vec::new(),
            body: None,
            timeout_ms: 5000,
        }
    }
}

impl HttpRequest {
    pub fn get(url: &str) -> Self {
        Self {
            method: "GET".into(),
            url: url.into(),
            ..Default::default()
        }
    }

    pub fn post(url: &str, body: Vec<u8>) -> Self {
        Self {
            method: "POST".into(),
            url: url.into(),
            body: Some(body),
            ..Default::default()
        }
    }

    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// HTTP 状态码
    pub status: u16,
    /// 响应头列表
    pub headers: Vec<(String, String)>,
    /// 原始响应体字节
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// 尝试将响应体解析为 UTF-8 字符串
    pub fn body_as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.body)
    }

    /// 尝试将响应体解析为字符串（lossy）
    pub fn body_as_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// 状态码是否在 2xx 范围内
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}
