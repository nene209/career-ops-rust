//! The custom client against a real (but local) OpenAI-compatible endpoint.
//!
//! These tests exist because the client's most consequential choices are only
//! visible on the wire: `/chat/completions` instead of `/responses`, no
//! `Authorization` header when the key is empty, and no `response_format` when
//! JSON mode is off. A fake backend above the transport cannot show any of
//! them, so this file binds a TCP listener on the loopback interface and
//! answers the paths `rig` calls.
//!
//! Nothing here reaches the network. `custom_live.rs` is the test that talks to
//! a real Ollama or `llama-server`, and it is gated by `CAREER_OPS_LIVE=1`.
//!
//! **If your shell exports `HTTP_PROXY`/`ALL_PROXY`,** run these with
//! `NO_PROXY=127.0.0.1,localhost`: `reqwest` honours the proxy variables even
//! for a loopback address, so the requests would go to the proxy instead of
//! this listener.
//!
//! Where the environment forbids `bind(2)` (some sandboxes do), each test
//! reports a skip instead of failing. A skip here means the environment, not
//! the code — a green run in a locked-down shell proves nothing.

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use career_ops_core::llm::{LlmConfig, LlmError, LlmProvider};

/// One request the fake endpoint answered.
#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    headers: String,
    body: String,
}

impl Recorded {
    fn header(&self, name: &str) -> Option<String> {
        let prefix = format!("{}:", name.to_ascii_lowercase());
        self.headers.lines().find_map(|line| {
            let lower = line.to_ascii_lowercase();
            lower
                .starts_with(&prefix)
                .then(|| line[prefix.len()..].trim().to_string())
        })
    }
}

type Responder = Arc<dyn Fn(&str) -> (u16, String) + Send + Sync>;

struct FakeEndpoint {
    base_url: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
}

impl FakeEndpoint {
    async fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().await.clone()
    }
}

/// What a healthy OpenAI-compatible server answers.
fn openai(path: &str) -> (u16, String) {
    if path.ends_with("/models") {
        (
            200,
            r#"{"object":"list","data":[
                 {"id":"llama3.2","object":"model","created":0,"owned_by":"local"}
               ]}"#
            .to_string(),
        )
    } else if path.ends_with("/chat/completions") {
        (200, chat_completion("pong", 3, 2))
    } else {
        (
            404,
            r#"{"error":{"message":"no such route","type":"invalid_request_error"}}"#.to_string(),
        )
    }
}

fn chat_completion(text: &str, prompt_tokens: u64, completion_tokens: u64) -> String {
    format!(
        r#"{{"id":"chatcmpl-1","object":"chat.completion","created":0,
            "model":"llama3.2",
            "choices":[{{"index":0,
                "message":{{"role":"assistant","content":"{text}"}},
                "finish_reason":"stop"}}],
            "usage":{{"prompt_tokens":{prompt_tokens},
                      "completion_tokens":{completion_tokens},
                      "total_tokens":{}}}}}"#,
        prompt_tokens + completion_tokens
    )
}

/// Bind an ephemeral loopback port, or `None` where the environment forbids
/// it.
///
/// A skip is printed so a green run in a restricted environment is not
/// mistaken for a pass.
async fn bind_loopback(test: &str) -> Option<TcpListener> {
    match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => Some(listener),
        Err(error) => {
            eprintln!("skipping {test}: cannot bind a loopback port ({error})");
            None
        }
    }
}

/// Start a listener on an ephemeral loopback port.
async fn spawn(
    test: &str,
    responder: impl Fn(&str) -> (u16, String) + Send + Sync + 'static,
) -> Option<FakeEndpoint> {
    let listener = bind_loopback(test).await?;
    let port = listener.local_addr().expect("local address").port();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&requests);
    let responder: Responder = Arc::new(responder);

    tokio::spawn(async move {
        // The test runtime ends the loop; a few connections are all any test
        // needs.
        while let Ok((mut stream, _)) = listener.accept().await {
            let responder = Arc::clone(&responder);
            let recorded = Arc::clone(&recorded);
            tokio::spawn(async move {
                let Some(request) = read_request(&mut stream).await else {
                    return;
                };
                let (status, body) = responder(&request.path);
                recorded.lock().await.push(request.clone());

                let response = format!(
                    "HTTP/1.1 {status} {}\r\n\
                     Content-Type: application/json\r\n\
                     Content-Length: {}\r\n\
                     Connection: close\r\n\r\n{body}",
                    if status == 200 { "OK" } else { "Error" },
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });

    Some(FakeEndpoint {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        requests,
    })
}

async fn read_request(stream: &mut TcpStream) -> Option<Recorded> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4_096];
    let header_end = loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
    };

    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split_whitespace();
    let method = request_line.next()?.to_string();
    let path = request_line.next()?.to_string();
    let headers = lines.collect::<Vec<_>>().join("\n");

    let length = headers
        .lines()
        .find_map(|line| {
            let lower = line.to_ascii_lowercase();
            lower
                .strip_prefix("content-length:")
                .map(|value| value.trim().to_string())
        })
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);

    let body_start = header_end + 4;
    while buffer.len() < body_start + length {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = String::from_utf8_lossy(&buffer[body_start..]).to_string();

    Some(Recorded {
        method,
        path,
        headers,
        body,
    })
}

fn local(base_url: &str, json_mode: bool) -> LlmConfig {
    LlmConfig {
        provider: LlmProvider::Custom,
        model: "llama3.2".into(),
        base_url: base_url.into(),
        json_mode,
        ..LlmConfig::default()
    }
}

#[tokio::test]
async fn a_keyless_call_reaches_chat_completions_without_authorization() {
    let Some(endpoint) = spawn(
        "a_keyless_call_reaches_chat_completions_without_authorization",
        openai,
    )
    .await
    else {
        return;
    };
    let config = local(&endpoint.base_url, false);

    let client = config
        .build_client(&zeroize::Zeroizing::new(String::new()))
        .expect("a local endpoint needs no key");

    let completion = client
        .complete_text("ping", None, false, None)
        .await
        .expect("the fake endpoint answers");

    assert_eq!(completion.text, "pong");
    assert_eq!(
        completion.usage.input_tokens,
        Some(3),
        "the reported counters are kept"
    );
    assert_eq!(completion.usage.total_tokens, Some(5));
    assert_eq!(
        completion.usage.cost_usd, None,
        "a self-hosted model has no list price"
    );

    let requests = endpoint.requests().await;
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.path, "/v1/chat/completions",
        "the Chat route, not the Responses API"
    );
    assert_eq!(
        request.header("authorization"),
        None,
        "an empty key must not send a bearer token"
    );
    assert!(request.body.contains("llama3.2"), "{}", request.body);
    assert!(request.body.contains("ping"), "{}", request.body);
}

#[tokio::test]
async fn json_mode_is_the_only_thing_that_sends_response_format() {
    let Some(endpoint) = spawn(
        "json_mode_is_the_only_thing_that_sends_response_format",
        openai,
    )
    .await
    else {
        return;
    };

    local(&endpoint.base_url, true)
        .build_client(&zeroize::Zeroizing::new(String::new()))
        .expect("client")
        .complete_text("ping", None, true, None)
        .await
        .expect("json mode call");

    local(&endpoint.base_url, false)
        .build_client(&zeroize::Zeroizing::new(String::new()))
        .expect("client")
        .complete_text("ping", None, true, None)
        .await
        .expect("prompt-only call");

    let requests = endpoint.requests().await;
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0].body.contains("response_format"),
        "JSON mode asks the provider to constrain the reply: {}",
        requests[0].body
    );
    assert!(
        !requests[1].body.contains("response_format"),
        "a prompt-only endpoint must not receive the field it may reject: {}",
        requests[1].body
    );
}

#[tokio::test]
async fn a_stored_key_is_sent_as_a_bearer_token() {
    let Some(endpoint) = spawn("a_stored_key_is_sent_as_a_bearer_token", openai).await else {
        return;
    };

    local(&endpoint.base_url, false)
        .build_client(&zeroize::Zeroizing::new("sk-local".into()))
        .expect("client")
        .complete_text("ping", None, false, None)
        .await
        .expect("call");

    let requests = endpoint.requests().await;
    assert_eq!(
        requests[0].header("authorization").as_deref(),
        Some("Bearer sk-local"),
        "a key the user stored must reach the server"
    );
}

#[tokio::test]
async fn testing_the_connection_asks_the_models_route() {
    let Some(endpoint) = spawn("testing_the_connection_asks_the_models_route", openai).await else {
        return;
    };

    local(&endpoint.base_url, true)
        .build_client(&zeroize::Zeroizing::new(String::new()))
        .expect("client")
        .verify()
        .await
        .expect("the fake endpoint lists its models");

    let requests = endpoint.requests().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/models");
    assert_eq!(
        requests[0].method, "GET",
        "verification is a read, not a completion"
    );
}

#[tokio::test]
async fn a_rejected_key_is_reported_as_an_authentication_failure() {
    let Some(endpoint) = spawn(
        "a_rejected_key_is_reported_as_an_authentication_failure",
        |path| {
            if path.ends_with("/chat/completions") {
                (
                    401,
                    r#"{"error":{"message":"bad key","type":"invalid_request_error"}}"#.to_string(),
                )
            } else {
                openai(path)
            }
        },
    )
    .await
    else {
        return;
    };

    let error = local(&endpoint.base_url, false)
        .build_client(&zeroize::Zeroizing::new("sk-wrong".into()))
        .expect("client")
        .complete_text("ping", None, false, None)
        .await
        .expect_err("the server refuses the key");

    assert!(
        matches!(error, LlmError::InvalidAuthentication { .. }),
        "{error:?}"
    );
    assert_eq!(error.kind(), "invalid_authentication");
    assert!(!error.is_retryable());
}

#[tokio::test]
async fn an_unreachable_endpoint_is_a_transport_failure() {
    // Bind, learn the port, then drop the listener: nothing answers.
    let Some(listener) = bind_loopback("an_unreachable_endpoint_is_a_transport_failure").await
    else {
        return;
    };
    let port = listener.local_addr().expect("address").port();
    drop(listener);

    let error = local(&format!("http://127.0.0.1:{port}/v1"), false)
        .build_client(&zeroize::Zeroizing::new(String::new()))
        .expect("client")
        .complete_text("ping", None, false, None)
        .await
        .expect_err("nothing is listening");

    assert!(
        matches!(error, LlmError::Transport(_)),
        "a refused connection is transport, not a provider reply: {error:?}"
    );
}
