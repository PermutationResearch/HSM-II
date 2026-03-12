//! Streaming Handler with Thinking/Reasoning Support
//!
//! Supports models like DeepSeek-R1 that output <think> tags for reasoning

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Streaming event types
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StreamEvent {
    /// Text content (regular response)
    Content { text: String },
    /// Thinking/reasoning block (e.g., <think> tags)
    Thinking { text: String },
    /// Tool call start
    ToolCallStart { id: String, name: String },
    /// Tool call arguments (streaming JSON)
    ToolCallArgs { id: String, args: String },
    /// Tool call complete
    ToolCallEnd { id: String },
    /// Error during streaming
    Error { message: String },
    /// Stream complete
    Done,
}

/// Thinking block with metadata
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThinkingBlock {
    pub content: String,
    pub start_time: u64,
    pub end_time: Option<u64>,
    pub is_complete: bool,
}

impl ThinkingBlock {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            start_time: super::now(),
            end_time: None,
            is_complete: false,
        }
    }

    pub fn append(&mut self, text: &str) {
        self.content.push_str(text);
    }

    pub fn complete(&mut self) {
        self.is_complete = true;
        self.end_time = Some(super::now());
    }

    pub fn duration_ms(&self) -> Option<u64> {
        self.end_time.map(|end| (end - self.start_time) * 1000)
    }
}

impl Default for ThinkingBlock {
    fn default() -> Self {
        Self::new()
    }
}

/// Streaming handler that processes tokens with thinking support
pub struct StreamingHandler {
    tx: mpsc::Sender<StreamEvent>,
    in_thinking: bool,
    thinking_buffer: String,
    content_buffer: String,
    strip_thinking: bool,
}

impl StreamingHandler {
    pub fn new(tx: mpsc::Sender<StreamEvent>, strip_thinking: bool) -> Self {
        Self {
            tx,
            in_thinking: false,
            thinking_buffer: String::new(),
            content_buffer: String::new(),
            strip_thinking,
        }
    }

    /// Process a token from the stream
    pub async fn process_token(
        &mut self,
        token: &str,
    ) -> Result<(), mpsc::error::SendError<StreamEvent>> {
        // Check for think tag start/end
        if token.contains("<think>") {
            self.in_thinking = true;
            // Send any pending content before switching to thinking
            if !self.content_buffer.is_empty() {
                self.tx
                    .send(StreamEvent::Content {
                        text: self.content_buffer.clone(),
                    })
                    .await?;
                self.content_buffer.clear();
            }
            // Extract content after <think>
            let after = token.split("<think>").nth(1).unwrap_or("");
            if !after.is_empty() {
                self.thinking_buffer.push_str(after);
            }
            return Ok(());
        }

        if token.contains("</think>") {
            self.in_thinking = false;
            // Extract content before </think>
            let before = token.split("</think>").next().unwrap_or("");
            self.thinking_buffer.push_str(before);

            // Send thinking block
            if !self.thinking_buffer.is_empty() {
                self.tx
                    .send(StreamEvent::Thinking {
                        text: self.thinking_buffer.clone(),
                    })
                    .await?;
                self.thinking_buffer.clear();
            }

            // Send any content after </think>
            let after = token.split("</think>").nth(1).unwrap_or("");
            if !after.is_empty() {
                if self.strip_thinking {
                    self.tx
                        .send(StreamEvent::Content {
                            text: after.to_string(),
                        })
                        .await?;
                } else {
                    self.content_buffer.push_str(after);
                }
            }
            return Ok(());
        }

        // Normal token processing
        if self.in_thinking {
            self.thinking_buffer.push_str(token);
        } else {
            self.content_buffer.push_str(token);
        }

        // Stream content immediately (for low latency)
        if !self.in_thinking && !self.content_buffer.is_empty() {
            self.tx
                .send(StreamEvent::Content {
                    text: self.content_buffer.clone(),
                })
                .await?;
            self.content_buffer.clear();
        }

        Ok(())
    }

    /// Flush any remaining buffers
    pub async fn flush(&mut self) -> Result<(), mpsc::error::SendError<StreamEvent>> {
        if !self.content_buffer.is_empty() {
            self.tx
                .send(StreamEvent::Content {
                    text: self.content_buffer.clone(),
                })
                .await?;
            self.content_buffer.clear();
        }

        if !self.thinking_buffer.is_empty() {
            self.tx
                .send(StreamEvent::Thinking {
                    text: self.thinking_buffer.clone(),
                })
                .await?;
            self.thinking_buffer.clear();
        }

        self.tx.send(StreamEvent::Done).await
    }

    /// Check if currently in thinking mode
    pub fn is_thinking(&self) -> bool {
        self.in_thinking
    }

    /// Get current thinking content
    pub fn thinking_content(&self) -> &str {
        &self.thinking_buffer
    }
}

/// Async streaming receiver
pub struct StreamReceiver {
    rx: mpsc::Receiver<StreamEvent>,
    collected_content: String,
    collected_thinking: String,
}

impl StreamReceiver {
    pub fn new(rx: mpsc::Receiver<StreamEvent>) -> Self {
        Self {
            rx,
            collected_content: String::new(),
            collected_thinking: String::new(),
        }
    }

    /// Receive next event
    pub async fn next(&mut self) -> Option<StreamEvent> {
        match self.rx.recv().await {
            Some(event) => {
                match &event {
                    StreamEvent::Content { text } => {
                        self.collected_content.push_str(text);
                    }
                    StreamEvent::Thinking { text } => {
                        self.collected_thinking.push_str(text);
                    }
                    _ => {}
                }
                Some(event)
            }
            None => None,
        }
    }

    /// Get collected content
    pub fn content(&self) -> &str {
        &self.collected_content
    }

    /// Get collected thinking
    pub fn thinking(&self) -> &str {
        &self.collected_thinking
    }

    /// Take collected content (clears internal buffer)
    pub fn take_content(&mut self) -> String {
        std::mem::take(&mut self.collected_content)
    }

    /// Take collected thinking (clears internal buffer)
    pub fn take_thinking(&mut self) -> String {
        std::mem::take(&mut self.collected_thinking)
    }
}

/// Builder for streaming configuration
pub struct StreamingConfig {
    pub buffer_size: usize,
    pub strip_thinking: bool,
    pub enable_tool_calls: bool,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            buffer_size: 1024,
            strip_thinking: false,
            enable_tool_calls: true,
        }
    }
}

/// Create a streaming channel pair
pub fn create_streaming_channel(config: &StreamingConfig) -> (StreamingHandler, StreamReceiver) {
    let (tx, rx) = mpsc::channel(config.buffer_size);
    let handler = StreamingHandler::new(tx, config.strip_thinking);
    let receiver = StreamReceiver::new(rx);
    (handler, receiver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_thinking_detection() {
        let config = StreamingConfig::default();
        let (mut handler, mut receiver) = create_streaming_channel(&config);

        handler.process_token("Hello ").await.unwrap();
        handler.process_token("<think>").await.unwrap();
        handler.process_token("reasoning...").await.unwrap();
        handler.process_token("</think>").await.unwrap();
        handler.process_token(" world!").await.unwrap();
        handler.flush().await.unwrap();

        let mut events = Vec::new();
        while let Some(event) = receiver.next().await {
            if matches!(event, StreamEvent::Done) {
                break;
            }
            events.push(event);
        }

        assert!(events
            .iter()
            .any(|e| matches!(e, StreamEvent::Content { text } if text == "Hello ")));
        assert!(events
            .iter()
            .any(|e| matches!(e, StreamEvent::Thinking { text } if text == "reasoning...")));
        assert!(events
            .iter()
            .any(|e| matches!(e, StreamEvent::Content { text } if text == " world!")));
    }
}
