//! SSE passthrough stream that scans upstream chunks for the final `usage`
//! event and records it into the database when the stream ends.

use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};

use axum::body::Bytes;
use futures::Stream;
use serde_json::Value;

use crate::database::Db;

/// Forwards upstream SSE bytes while extracting token usage from the last chunk.
pub struct UsageTrackingStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    db: Db,
    key_id: String,
    model: String,
    provider: String,
    started: Instant,
    prompt_tokens: i64,
    completion_tokens: i64,
    recorded: bool,
}

impl UsageTrackingStream {
    pub fn new(
        inner: reqwest::Response,
        db: Db,
        key_id: String,
        model: String,
        provider: String,
    ) -> Self {
        Self {
            inner: Box::pin(inner.bytes_stream()),
            db,
            key_id,
            model,
            provider,
            started: Instant::now(),
            prompt_tokens: 0,
            completion_tokens: 0,
            recorded: false,
        }
    }

    /// Scan one SSE chunk for `data: {...usage...}` payloads.
    fn scan(&mut self, chunk: &[u8]) {
        for line in chunk.split(|b| *b == b'\n') {
            let line = std::str::from_utf8(line).unwrap_or("");
            let text = line.strip_prefix("data:").unwrap_or(line).trim();
            if text == "[DONE]" || !text.contains("usage") {
                continue;
            }
            let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else {
                continue;
            };
            if let Some(usage) = map.get("usage").and_then(|u| u.as_object()) {
                self.prompt_tokens = usage
                    .get("prompt_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                self.completion_tokens = usage
                    .get("completion_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
            }
        }
    }

    /// Insert the accumulated usage row exactly once (called on stream end).
    fn record(&mut self) {
        if self.recorded {
            return;
        }
        self.recorded = true;
        let latency = self.started.elapsed().as_millis() as i64;
        let _ = self.db.insert_usage_blocking(
            &self.key_id,
            &self.model,
            &self.provider,
            self.prompt_tokens,
            self.completion_tokens,
            latency,
            200,
        );
    }
}

impl Stream for UsageTrackingStream {
    type Item = Result<Bytes, axum::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                self.scan(&bytes);
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(Err(e))) => {
                self.record();
                Poll::Ready(Some(Err(axum::Error::new(e))))
            }
            Poll::Ready(None) => {
                self.record();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
