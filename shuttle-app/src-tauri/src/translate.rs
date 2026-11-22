//! Opt-in message translation through the user's own OpenRouter account. Nothing is translated,
//! and no text leaves the device, until an API key is set in Settings. Results are shown and
//! never stored.

use crate::config::AiConfig;
use serde::Serialize;
use serde_json::{json, Value};

pub const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
pub const DEFAULT_MODEL: &str = "google/gemini-2.5-flash";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranslateError {
    /// No API key in Settings: the UI opens that field.
    NotConfigured,
    Failed {
        message: String,
    },
}

fn failed(message: impl Into<String>) -> TranslateError {
    TranslateError::Failed {
        message: message.into(),
    }
}

/// One translated message of a `translate_text` call. Exactly one of `text` / `error` is set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Translation {
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn build_body(cfg: &AiConfig, text: &str, target: &str) -> Result<Value, TranslateError> {
    if cfg.openrouter_api_key.trim().is_empty() {
        return Err(TranslateError::NotConfigured);
    }
    let model = match cfg.model.trim() {
        "" => DEFAULT_MODEL,
        m => m,
    };
    let target = match target.trim() {
        "" => "en",
        t => t,
    };
    Ok(json!({
        "model": model,
        "temperature": 0,
        "messages": [
            {
                "role": "system",
                "content": format!(
                    "You translate chat messages. Translate the user's message into the language with code \"{target}\". Reply with the translation only, no notes, quotes or explanation. Keep emoji, names, links and line breaks. If the message is already in that language, return it unchanged."
                )
            },
            { "role": "user", "content": text }
        ]
    }))
}

fn api_error(v: &Value) -> Option<String> {
    let e = v.get("error")?;
    e.get("message")
        .and_then(|m| m.as_str())
        .or_else(|| e.as_str())
        .map(str::to_string)
}

/// Send one text to OpenRouter.
pub async fn translate(
    client: &reqwest::Client,
    cfg: &AiConfig,
    text: &str,
    target: &str,
) -> Result<String, TranslateError> {
    translate_at(client, OPENROUTER_URL, cfg, text, target).await
}

pub async fn translate_at(
    client: &reqwest::Client,
    url: &str,
    cfg: &AiConfig,
    text: &str,
    target: &str,
) -> Result<String, TranslateError> {
    let body = build_body(cfg, text, target)?;
    let resp = client
        .post(url)
        .bearer_auth(cfg.openrouter_api_key.trim())
        .header("X-Title", "Shuttle")
        .json(&body)
        .send()
        .await
        .map_err(|e| failed(format!("Could not reach OpenRouter: {e}")))?;
    let status = resp.status();
    let v: Value = resp
        .json()
        .await
        .map_err(|_| failed(format!("OpenRouter sent no usable answer ({status})")))?;
    if !status.is_success() {
        return Err(failed(
            api_error(&v).unwrap_or_else(|| format!("Translation failed ({status})")),
        ));
    }
    v.pointer("/choices/0/message/content")
        .and_then(|t| t.as_str())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            failed(api_error(&v).unwrap_or_else(|| "OpenRouter sent an unexpected answer".into()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    fn cfg(key: &str, model: &str) -> AiConfig {
        AiConfig {
            openrouter_api_key: key.into(),
            model: model.into(),
        }
    }

    #[test]
    fn body_names_the_model_and_target_and_falls_back_to_defaults() {
        let b = build_body(&cfg("k", " openai/gpt-4o-mini "), "hola", "de").unwrap();
        assert_eq!(b["model"], "openai/gpt-4o-mini");
        assert_eq!(b["messages"][1]["content"], "hola");
        assert!(b["messages"][0]["content"].as_str().unwrap().contains("\"de\""));
        let b = build_body(&cfg("k", ""), "x", " ").unwrap();
        assert_eq!(b["model"], DEFAULT_MODEL);
        assert!(b["messages"][0]["content"].as_str().unwrap().contains("\"en\""));
    }

    #[test]
    fn no_key_sends_nothing() {
        assert_eq!(
            build_body(&cfg("  ", "m"), "hola", "en"),
            Err(TranslateError::NotConfigured)
        );
        assert_eq!(
            serde_json::to_value(TranslateError::NotConfigured).unwrap(),
            json!({"kind": "not_configured"})
        );
    }

    /// Serves one canned answer on a local port and hands back the auth header and request body.
    fn one_shot_server(
        status: &'static str,
        answer: &'static str,
    ) -> (String, std::thread::JoinHandle<(String, Value)>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api/v1/chat/completions", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            assert!(
                request_line.starts_with("POST /api/v1/chat/completions "),
                "{request_line}"
            );
            let mut length = 0usize;
            let mut auth = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
                if lower.starts_with("authorization:") {
                    auth = line["authorization:".len()..].trim().to_string();
                }
            }
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body).unwrap();
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
            (auth, serde_json::from_slice(&body).unwrap())
        });
        (url, handle)
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    #[tokio::test]
    async fn translates_through_an_openrouter_chat_completion() {
        let (url, server) = one_shot_server(
            "200 OK",
            r#"{"choices":[{"message":{"role":"assistant","content":" hello \n"}}]}"#,
        );
        let out = translate_at(&client(), &url, &cfg("sk-1", "m/x"), "hola", "en").await;
        assert_eq!(out, Ok("hello".to_string()));
        let (auth, body) = server.join().unwrap();
        assert_eq!(auth, "Bearer sk-1");
        assert_eq!(body["model"], "m/x");
    }

    #[tokio::test]
    async fn api_errors_are_reported_with_the_providers_reason() {
        let (url, server) = one_shot_server(
            "401 Unauthorized",
            r#"{"error":{"code":401,"message":"No auth credentials found"}}"#,
        );
        let out = translate_at(&client(), &url, &cfg("bad", ""), "hola", "en").await;
        assert_eq!(out, Err(failed("No auth credentials found")));
        server.join().unwrap();
    }
}
