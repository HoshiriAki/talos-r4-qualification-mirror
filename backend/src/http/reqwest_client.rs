use system_core::transport::http_client::{HttpClient, HttpRequest, HttpResponse};

pub struct ReqwestHttpClient {
    client: reqwest::Client,
}

impl ReqwestHttpClient {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("ReqwestHttpClient: failed to build HTTP client");
        Self { client }
    }
}

impl HttpClient for ReqwestHttpClient {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, String> {
        let mut req = match request.method.as_str() {
            "POST" => self.client.post(&request.url),
            "GET" => self.client.get(&request.url),
            _ => return Err("不支持的 HTTP 方法".into()),
        };

        for (k, v) in &request.headers {
            req = req.header(k.as_str(), v.as_str());
        }

        let req_timeout = if request.timeout_ms > 0 {
            std::time::Duration::from_millis(request.timeout_ms)
        } else {
            std::time::Duration::from_secs(30)
        };

        let handle = tokio::runtime::Handle::try_current()
            .map_err(|e| format!("SF_NETWORK_ERROR: no tokio runtime: {}", e))?;

        let resp_future = req
            .body(request.body.unwrap_or_default())
            .timeout(req_timeout)
            .send();

        let resp = handle
            .block_on(resp_future)
            .map_err(|e| format!("SF_NETWORK_ERROR: {}", e))?;

        let status = resp.status().as_u16();
        let body = handle
            .block_on(resp.bytes())
            .map_err(|e| format!("SF_NETWORK_ERROR: {}", e))?;

        Ok(HttpResponse {
            status,
            body: body.to_vec(),
            headers: Vec::new(),
        })
    }

    fn ping(&self, base_url: &str) -> Result<bool, String> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|e| format!("Ping failed: no tokio runtime: {}", e))?;

        let resp = handle
            .block_on(self.client.head(base_url).send())
            .map_err(|e| format!("Ping failed: {}", e))?;
        Ok(resp.status().is_success())
    }
}
