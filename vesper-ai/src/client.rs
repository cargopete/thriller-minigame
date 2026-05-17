use anyhow::{bail, Result};
use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::auth::Auth;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Events emitted by the streaming pipeline to the UI layer.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    Delta(String),
    Done,
    Error(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: content.into() }
    }
}

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: &'a [Message],
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
}

// Anthropic SSE delta shape: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"…"}}
#[derive(Deserialize)]
struct ContentBlockDelta {
    delta: TextDelta,
}

#[derive(Deserialize)]
struct TextDelta {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

pub struct AnthropicClient {
    http: Client,
    auth: Auth,
}

impl AnthropicClient {
    pub fn new(auth: Auth) -> Self {
        Self { http: Client::new(), auth }
    }

    /// Stream a message from the API. Sends `StreamEvent`s on `tx` until `Done` or `Error`.
    pub async fn stream(
        &self,
        model: &str,
        system: Option<&str>,
        messages: &[Message],
        max_tokens: u32,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<()> {
        let body = Request { model, max_tokens, messages, stream: true, system };

        let mut delay_ms = 2_000u64;
        let response = loop {
            let r = self.auth
                .apply(self.http.post(API_URL))
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await?;
            if r.status().as_u16() != 429 || delay_ms > 16_000 {
                break r;
            }
            eprintln!("[narrator] rate limited, retrying in {delay_ms}ms…");
            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
            delay_ms *= 2;
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let msg = format!("API {status}: {body}");
            let _ = tx.send(StreamEvent::Error(msg.clone()));
            bail!(msg);
        }

        let mut stream = response.bytes_stream().eventsource();

        while let Some(item) = stream.next().await {
            match item {
                Ok(ev) => match ev.event.as_str() {
                    "content_block_delta" => {
                        if let Ok(d) = serde_json::from_str::<ContentBlockDelta>(&ev.data) {
                            if d.delta.kind == "text_delta" && !d.delta.text.is_empty() {
                                if tx.send(StreamEvent::Delta(d.delta.text)).is_err() {
                                    break; // receiver dropped
                                }
                            }
                        }
                    }
                    "message_stop" => {
                        let _ = tx.send(StreamEvent::Done);
                        break;
                    }
                    _ => {} // ping, message_start, content_block_start/stop, message_delta
                },
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(e.to_string()));
                    break;
                }
            }
        }

        Ok(())
    }
}
