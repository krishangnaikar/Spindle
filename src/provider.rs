//! Bounded, single-turn provider requests. Credentials never enter serializable models.
use anyhow::{Result, bail, ensure};
use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderValue},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{str::FromStr, time::Duration};

const RESPONSE_LIMIT: usize = 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Openai,
    Anthropic,
    Openrouter,
}
impl FromStr for Provider {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "openai" => Ok(Self::Openai),
            "anthropic" => Ok(Self::Anthropic),
            "openrouter" => Ok(Self::Openrouter),
            _ => bail!("unsupported provider"),
        }
    }
}
pub struct CompletionRequest {
    pub provider: Provider,
    pub model: String,
    pub prompt: String,
    pub max_output_tokens: u32,
    pub free_only: bool,
}
impl CompletionRequest {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.model.is_empty()
                && self.model.len() <= 256
                && self
                    .model
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/._:-".contains(&b)),
            "invalid model identifier"
        );
        ensure!(
            !self.prompt.trim().is_empty() && self.prompt.len() <= 64 * 1024,
            "prompt must contain text and fit within 64 KiB"
        );
        ensure!(
            (1..=16384).contains(&self.max_output_tokens),
            "output token limit must be 1-16384"
        );
        if self.free_only {
            ensure!(
                self.provider == Provider::Openrouter,
                "free-only is supported only by OpenRouter"
            );
            ensure!(
                self.model == "openrouter/free"
                    || (self.model.ends_with(":free") && self.model.len() > 5),
                "free-only requires openrouter/free or a :free model"
            );
        }
        Ok(())
    }
}
#[derive(Serialize)]
pub struct Completion {
    pub provider: Provider,
    pub model: String,
    pub text: String,
    /// `stop`, `length`, or `incomplete`; truncation is never labeled complete.
    pub finish_reason: String,
    pub usage: Value,
}
fn endpoint(provider: Provider) -> &'static str {
    match provider {
        Provider::Openai => "https://api.openai.com/v1/responses",
        Provider::Anthropic => "https://api.anthropic.com/v1/messages",
        Provider::Openrouter => "https://openrouter.ai/api/v1/chat/completions",
    }
}
fn body(request: &CompletionRequest) -> Value {
    match request.provider {
        Provider::Openai => {
            json!({"model":request.model,"input":request.prompt,"max_output_tokens":request.max_output_tokens,"store":false,"stream":false})
        }
        Provider::Anthropic => {
            json!({"model":request.model,"messages":[{"role":"user","content":request.prompt}],"max_tokens":request.max_output_tokens,"stream":false})
        }
        Provider::Openrouter => {
            let mut value = json!({"model":request.model,"messages":[{"role":"user","content":request.prompt}],"max_tokens":request.max_output_tokens,"stream":false});
            if request.free_only {
                value["provider"] = json!({"max_price":{"prompt":0,"completion":0},"allow_fallbacks":false,"require_parameters":true});
            }
            value
        }
    }
}
fn client(timeout: Duration) -> Result<Client> {
    Client::builder()
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .build()
        .map_err(|_| anyhow::anyhow!("could not initialize provider transport"))
}
pub async fn complete(request: &CompletionRequest, api_key: &str) -> Result<Completion> {
    request.validate()?;
    send(
        request,
        api_key,
        endpoint(request.provider),
        Duration::from_secs(60),
    )
    .await
}
// Endpoint injection is private and used only by local mock tests. Public callers
// cannot redirect credentials to custom URLs or enable proxies/redirects/retries.
async fn send(
    request: &CompletionRequest,
    api_key: &str,
    url: &str,
    timeout: Duration,
) -> Result<Completion> {
    request.validate()?;
    ensure!(
        !api_key.is_empty()
            && api_key.len() <= 4096
            && api_key.bytes().all(|b| (33..=126).contains(&b)),
        "invalid API credential"
    );
    let mut authorization = HeaderValue::from_str(&format!("Bearer {api_key}"))
        .map_err(|_| anyhow::anyhow!("invalid API credential"))?;
    authorization.set_sensitive(true);
    let client = client(timeout)?;
    let mut builder = client
        .post(url)
        .header(AUTHORIZATION, authorization)
        .json(&body(request));
    if request.provider == Provider::Anthropic {
        builder = builder.header("anthropic-version", "2026-10-21");
    }
    // Cover headers AND the entire streamed body with one deadline.
    let operation = async {
        let mut response = builder.send().await.map_err(transport_error)?;
        ensure!(
            response.status().is_success(),
            "provider HTTP status {}",
            response.status().as_u16()
        );
        ensure!(
            response
                .content_length()
                .is_none_or(|n| n <= RESPONSE_LIMIT as u64),
            "provider response exceeds 1 MiB"
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            ensure!(
                chunk.len() <= RESPONSE_LIMIT - bytes.len(),
                "provider response exceeds 1 MiB"
            );
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("provider returned malformed JSON"))?;
        parse(request, &value)
    };
    tokio::time::timeout(timeout, operation)
        .await
        .map_err(|_| anyhow::anyhow!("provider request timed out; outcome unknown, not retried"))?
}
fn transport_error(error: reqwest::Error) -> anyhow::Error {
    if error.is_timeout() {
        anyhow::anyhow!("provider request timed out; outcome unknown, not retried")
    } else {
        anyhow::anyhow!("provider transport failed; outcome unknown, not retried")
    }
}
fn text(value: &Value) -> Result<&str> {
    value
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("provider response has malformed text fields"))
}
fn array(value: &Value) -> Result<&Vec<Value>> {
    value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("provider response has malformed content"))
}
fn parse(request: &CompletionRequest, value: &Value) -> Result<Completion> {
    ensure!(
        value.get("error").is_none_or(Value::is_null),
        "provider returned an error"
    );
    let model = text(&value["model"])?;
    ensure!(
        !model.is_empty() && model.len() <= 256 && !model.chars().any(char::is_control),
        "provider returned invalid model identifier"
    );
    let mut output = String::new();
    let finish = match request.provider {
        Provider::Openai => {
            let finish = match text(&value["status"])? {
                "completed" => "stop",
                "incomplete" => match value["incomplete_details"]["reason"].as_str() {
                    Some("max_output_tokens") => "length",
                    Some("content_filter") => bail!("provider refused the request"),
                    _ => "incomplete",
                },
                _ => bail!("provider returned a nonterminal or failed response"),
            };
            for item in array(&value["output"])? {
                match item["type"].as_str() {
                    Some("reasoning") => {}
                    Some("message") => {
                        ensure!(
                            item["role"] == "assistant",
                            "provider returned unexpected message role"
                        );
                        for part in array(&item["content"])? {
                            match part["type"].as_str() {
                                Some("output_text") => output.push_str(text(&part["text"])?),
                                Some("refusal") => bail!("provider refused the request"),
                                _ => bail!("provider returned unsupported non-text content"),
                            }
                        }
                    }
                    _ => bail!("provider returned unsupported tool or output content"),
                }
            }
            finish
        }
        Provider::Anthropic => {
            ensure!(
                value["type"] == "message" && value["role"] == "assistant",
                "provider returned malformed message"
            );
            ensure!(
                value["stop_details"]["type"] != "refusal",
                "provider refused the request"
            );
            let finish = match text(&value["stop_reason"])? {
                "end_turn" | "stop_sequence" => "stop",
                "max_tokens" => "length",
                "pause_turn" => "incomplete",
                "refusal" => bail!("provider refused the request"),
                _ => bail!("provider returned unsupported tool or stop reason"),
            };
            for part in array(&value["content"])? {
                match part["type"].as_str() {
                    Some("text") => output.push_str(text(&part["text"])?),
                    Some("thinking" | "redacted_thinking") => {}
                    _ => bail!("provider returned unsupported tool or content block"),
                }
            }
            finish
        }
        Provider::Openrouter => {
            let choices = array(&value["choices"])?;
            ensure!(
                choices.len() == 1,
                "provider returned unexpected number of choices"
            );
            let choice = &choices[0];
            let message = &choice["message"];
            ensure!(
                message["role"] == "assistant",
                "provider returned unexpected message role"
            );
            ensure!(
                message.get("refusal").is_none_or(Value::is_null),
                "provider refused the request"
            );
            ensure!(
                message
                    .get("tool_calls")
                    .is_none_or(|v| v.is_null() || v.as_array().is_some_and(Vec::is_empty))
                    && message.get("function_call").is_none_or(Value::is_null),
                "provider returned unsupported tool calls"
            );
            output.push_str(text(&message["content"])?);
            match text(&choice["finish_reason"])? {
                "stop" => "stop",
                "length" => "length",
                "content_filter" => bail!("provider refused the request"),
                _ => bail!("provider returned unsupported finish reason"),
            }
        }
    };
    ensure!(
        !output.is_empty() || finish != "stop",
        "provider returned no text"
    );
    // Copy numeric accounting only, never arbitrary provider metadata into logs/output.
    let mut usage = serde_json::Map::new();
    if let Some(raw) = value.get("usage").filter(|v| !v.is_null()) {
        ensure!(raw.is_object(), "provider returned malformed usage");
        for field in [
            "input_tokens",
            "output_tokens",
            "total_tokens",
            "prompt_tokens",
            "completion_tokens",
            "cache_creation_input_tokens",
            "cache_read_input_tokens",
        ] {
            if let Some(n) = raw.get(field) {
                ensure!(
                    n.as_u64().is_some(),
                    "provider returned malformed token usage"
                );
                usage.insert(field.into(), n.clone());
            }
        }
    }
    Ok(Completion {
        provider: request.provider,
        model: model.into(),
        text: output,
        finish_reason: finish.into(),
        usage: Value::Object(usage),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    fn request(provider: Provider) -> CompletionRequest {
        CompletionRequest {
            provider,
            model: "test-model".into(),
            prompt: "hello".into(),
            max_output_tokens: 32,
            free_only: false,
        }
    }
    fn success(provider: Provider) -> Value {
        match provider {
            Provider::Openai => {
                json!({"model":"test-model","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]}],"usage":{"input_tokens":2,"output_tokens":1}})
            }
            Provider::Anthropic => {
                json!({"model":"test-model","type":"message","role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"hello"}],"usage":{"input_tokens":2,"output_tokens":1}})
            }
            Provider::Openrouter => {
                json!({"model":"test-model","choices":[{"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}})
            }
        }
    }
    async fn server(
        response: Vec<u8>,
        delay: Duration,
    ) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut data = Vec::new();
            let header_end = loop {
                let mut chunk = [0; 1024];
                let n = socket.read(&mut chunk).await.unwrap();
                if n == 0 {
                    return data;
                }
                data.extend_from_slice(&chunk[..n]);
                if let Some(i) = data.windows(4).position(|p| p == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            let headers = String::from_utf8_lossy(&data[..header_end]).to_ascii_lowercase();
            let length = headers
                .lines()
                .find_map(|l| l.strip_prefix("content-length: "))
                .unwrap()
                .parse::<usize>()
                .unwrap();
            while data.len() < header_end + length {
                let mut chunk = [0; 1024];
                let n = socket.read(&mut chunk).await.unwrap();
                if n == 0 {
                    break;
                }
                data.extend_from_slice(&chunk[..n]);
            }
            tokio::time::sleep(delay).await;
            let _ = socket.write_all(&response).await;
            data
        });
        (url, task)
    }
    fn http(status: u16, body: &[u8]) -> Vec<u8> {
        let mut reply=format!("HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).into_bytes();
        reply.extend_from_slice(body);
        reply
    }
    #[test]
    fn validates_bounds_and_free_policy() {
        for provider in [Provider::Openai, Provider::Anthropic, Provider::Openrouter] {
            let mut r = request(provider);
            r.validate().unwrap();
            r.max_output_tokens = 0;
            assert!(r.validate().is_err());
            r.max_output_tokens = 16385;
            assert!(r.validate().is_err());
            r.max_output_tokens = 16384;
            r.validate().unwrap();
            r.prompt = "x".repeat(65536);
            r.validate().unwrap();
            r.prompt.push('x');
            assert!(r.validate().is_err());
            r.prompt = " ".into();
            assert!(r.validate().is_err());
            r.prompt = "ok".into();
            r.model = "x".repeat(257);
            assert!(r.validate().is_err());
            r.model = "openrouter/free".into();
            r.free_only = true;
            assert_eq!(r.validate().is_ok(), provider == Provider::Openrouter);
        }
        let mut r = request(Provider::Openrouter);
        r.free_only = true;
        for m in ["openrouter/free", "vendor/model:free"] {
            r.model = m.into();
            r.validate().unwrap();
        }
        for m in [
            "vendor/model",
            ":free",
            "vendor/model:free:paid",
            "evil\nmodel",
        ] {
            r.model = m.into();
            assert!(r.validate().is_err());
        }
    }
    #[test]
    fn payloads_pin_protocol_and_free_routing_without_alternate_models() {
        let r = request(Provider::Openai);
        assert_eq!(body(&r)["store"], false);
        assert_eq!(body(&r)["max_output_tokens"], 32);
        let mut r = request(Provider::Openrouter);
        r.free_only = true;
        r.model = "openrouter/free".into();
        let b = body(&r);
        assert_eq!(
            b["provider"]["max_price"],
            json!({"prompt":0,"completion":0})
        );
        assert_eq!(b["provider"]["allow_fallbacks"], false);
        assert_eq!(b["provider"]["require_parameters"], true);
        assert!(b.get("models").is_none());
        assert_eq!(
            endpoint(Provider::Anthropic),
            "https://api.anthropic.com/v1/messages"
        );
        assert!("other".parse::<Provider>().is_err());
        assert_eq!("openai".parse::<Provider>().unwrap(), Provider::Openai);
    }
    #[test]
    fn all_providers_normalize_text_and_numeric_usage_only() {
        for provider in [Provider::Openai, Provider::Anthropic, Provider::Openrouter] {
            let mut data = success(provider);
            data["usage"]["untrusted_metadata"] = json!("not copied");
            let result = parse(&request(provider), &data).unwrap();
            assert_eq!(result.text, "hello");
            assert_eq!(result.finish_reason, "stop");
            assert!(result.usage.get("untrusted_metadata").is_none());
        }
    }
    #[test]
    fn incomplete_and_length_are_not_complete() {
        let mut o = success(Provider::Openai);
        o["status"] = json!("incomplete");
        o["incomplete_details"] = json!({"reason":"max_output_tokens"});
        o["output"] = json!([]);
        assert_eq!(
            parse(&request(Provider::Openai), &o).unwrap().finish_reason,
            "length"
        );
        o["incomplete_details"] = json!({"reason":"unknown"});
        assert_eq!(
            parse(&request(Provider::Openai), &o).unwrap().finish_reason,
            "incomplete"
        );
        let mut a = success(Provider::Anthropic);
        a["stop_reason"] = json!("max_tokens");
        assert_eq!(
            parse(&request(Provider::Anthropic), &a)
                .unwrap()
                .finish_reason,
            "length"
        );
        let mut r = success(Provider::Openrouter);
        r["choices"][0]["finish_reason"] = json!("length");
        assert_eq!(
            parse(&request(Provider::Openrouter), &r)
                .unwrap()
                .finish_reason,
            "length"
        );
    }
    #[test]
    fn refuses_tools_refusals_missing_text_and_malformed_usage() {
        let mut o = success(Provider::Openai);
        o["output"][0]["type"] = json!("function_call");
        assert!(parse(&request(Provider::Openai), &o).is_err());
        o = success(Provider::Openai);
        o["output"][0]["content"][0] = json!({"type":"refusal","refusal":"private refusal detail"});
        assert_eq!(
            parse(&request(Provider::Openai), &o)
                .err()
                .unwrap()
                .to_string(),
            "provider refused the request"
        );
        let mut a = success(Provider::Anthropic);
        a["stop_details"] = json!({"type":"refusal"});
        assert!(parse(&request(Provider::Anthropic), &a).is_err());
        a = success(Provider::Anthropic);
        a["content"][0]["type"] = json!("tool_use");
        assert!(parse(&request(Provider::Anthropic), &a).is_err());
        let mut r = success(Provider::Openrouter);
        r["choices"][0]["message"]["tool_calls"] = json!([{}]);
        assert!(parse(&request(Provider::Openrouter), &r).is_err());
        for provider in [Provider::Openai, Provider::Anthropic, Provider::Openrouter] {
            assert!(parse(&request(provider), &json!({})).is_err());
            let mut data = success(provider);
            data["usage"] = json!({"input_tokens":-1});
            assert!(parse(&request(provider), &data).is_err());
        }
    }
    #[tokio::test]
    async fn mock_requests_use_bearer_and_anthropic_current_version() {
        for provider in [Provider::Openai, Provider::Anthropic, Provider::Openrouter] {
            let data = serde_json::to_vec(&success(provider)).unwrap();
            let (url, task) = server(http(200, &data), Duration::ZERO).await;
            let result = send(
                &request(provider),
                "test-placeholder",
                &url,
                Duration::from_secs(2),
            )
            .await
            .unwrap();
            assert_eq!(result.text, "hello");
            let raw = String::from_utf8(task.await.unwrap()).unwrap();
            let headers = raw.split("\r\n\r\n").next().unwrap().to_ascii_lowercase();
            assert!(headers.contains("authorization: bearer test-placeholder"));
            if provider == Provider::Anthropic {
                assert!(headers.contains("anthropic-version: 2026-10-21"));
            }
            let actual: Value =
                serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(actual, body(&request(provider)));
        }
    }
    #[tokio::test]
    async fn errors_never_return_body_credential_or_transport_url() {
        for status in [401, 429, 500] {
            let (url, task) = server(
                http(status, b"test-placeholder secret response details"),
                Duration::ZERO,
            )
            .await;
            let error = send(
                &request(Provider::Openai),
                "test-placeholder",
                &url,
                Duration::from_secs(2),
            )
            .await
            .err()
            .unwrap();
            assert_eq!(
                format!("{error:#}"),
                format!("provider HTTP status {status}")
            );
            task.await.unwrap();
        }
        let error = send(
            &request(Provider::Openai),
            "bad\ncredential",
            "http://127.0.0.1:1",
            Duration::from_secs(1),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.to_string(), "invalid API credential");
    }
    #[tokio::test]
    async fn redirects_are_not_followed() {
        let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let response = format!(
            "HTTP/1.1 307 Redirect\r\nLocation: http://{}\r\nContent-Length: 0\r\n\r\n",
            target.local_addr().unwrap()
        )
        .into_bytes();
        let (url, task) = server(response, Duration::ZERO).await;
        let error = send(
            &request(Provider::Openai),
            "test-placeholder",
            &url,
            Duration::from_secs(2),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.to_string(), "provider HTTP status 307");
        task.await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(30), target.accept())
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn enforces_response_cap_for_declared_and_chunked_bodies() {
        for chunked in [false, true] {
            let response = if chunked {
                let data = vec![b'x'; RESPONSE_LIMIT + 1];
                let mut r = format!(
                    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",
                    data.len()
                )
                .into_bytes();
                r.extend(data);
                r.extend_from_slice(b"\r\n0\r\n\r\n");
                r
            } else {
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    RESPONSE_LIMIT + 1
                )
                .into_bytes()
            };
            let (url, task) = server(response, Duration::ZERO).await;
            let error = send(
                &request(Provider::Openai),
                "test-placeholder",
                &url,
                Duration::from_secs(2),
            )
            .await
            .err()
            .unwrap();
            assert_eq!(error.to_string(), "provider response exceeds 1 MiB");
            task.await.unwrap();
        }
    }
    #[tokio::test]
    async fn timeout_and_malformed_json_have_fixed_errors() {
        let (url, task) = server(http(200, b"not-json secret detail"), Duration::ZERO).await;
        assert_eq!(
            send(
                &request(Provider::Openai),
                "test-placeholder",
                &url,
                Duration::from_secs(2)
            )
            .await
            .err()
            .unwrap()
            .to_string(),
            "provider returned malformed JSON"
        );
        task.await.unwrap();
        let (url, task) = server(http(200, b"{}"), Duration::from_millis(200)).await;
        let error = send(
            &request(Provider::Openai),
            "test-placeholder",
            &url,
            Duration::from_millis(20),
        )
        .await
        .err()
        .unwrap();
        assert!(error.to_string().contains("timed out"));
        task.abort();
    }
    #[tokio::test]
    async fn accepts_exact_response_size_limit() {
        let mut payload = serde_json::to_vec(&success(Provider::Openai)).unwrap();
        payload.resize(RESPONSE_LIMIT, b' ');
        let (url, task) = server(http(200, &payload), Duration::ZERO).await;
        let result = send(
            &request(Provider::Openai),
            "test-placeholder",
            &url,
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(result.text, "hello");
        task.await.unwrap();
    }
    #[tokio::test]
    async fn deadline_includes_body_after_headers_arrive() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 4096];
            let _ = socket.read(&mut bytes).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(2)).await;
        });
        let result = send(
            &request(Provider::Openai),
            "test-placeholder",
            &url,
            Duration::from_millis(50),
        )
        .await;
        assert!(result.err().unwrap().to_string().contains("timed out"));
        task.abort();
    }
}
