//! Message Queue - Two modes: one-at-a-time or all-at-once

use super::Message;

/// Queue mode for message handling
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QueueMode {
    /// Process one message at a time
    OneAtATime,
    /// Process all queued messages at once
    AllAtOnce,
}

impl Default for QueueMode {
    fn default() -> Self {
        QueueMode::OneAtATime
    }
}

/// Message queue
pub struct MessageQueue {
    messages: Vec<Message>,
    mode: QueueMode,
}

impl MessageQueue {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            mode: QueueMode::default(),
        }
    }

    pub fn with_mode(mut self, mode: QueueMode) -> Self {
        self.mode = mode;
        self
    }

    /// Add message to queue
    pub fn push(&mut self, msg: Message) {
        self.messages.push(msg);
    }

    /// Add multiple messages
    pub fn extend(&mut self, msgs: Vec<Message>) {
        self.messages.extend(msgs);
    }

    /// Get messages based on mode
    pub fn drain(&mut self) -> Vec<Message> {
        match self.mode {
            QueueMode::OneAtATime => {
                if self.messages.is_empty() {
                    Vec::new()
                } else {
                    vec![self.messages.remove(0)]
                }
            }
            QueueMode::AllAtOnce => std::mem::take(&mut self.messages),
        }
    }

    /// Peek at next message(s) without removing
    pub fn peek(&self) -> Vec<&Message> {
        match self.mode {
            QueueMode::OneAtATime => self.messages.first().into_iter().collect(),
            QueueMode::AllAtOnce => self.messages.iter().collect(),
        }
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Get queue length
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Clear queue
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Set mode
    pub fn set_mode(&mut self, mode: QueueMode) {
        self.mode = mode;
    }

    /// Get current mode
    pub fn mode(&self) -> QueueMode {
        self.mode
    }
}

impl Default for MessageQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{now, Role};
    use super::*;

    fn make_msg(content: &str) -> Message {
        Message {
            role: Role::User,
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
            attachments: Vec::new(),
            timestamp: now(),
        }
    }

    #[test]
    fn test_one_at_a_time() {
        let mut queue = MessageQueue::new().with_mode(QueueMode::OneAtATime);

        queue.push(make_msg("first"));
        queue.push(make_msg("second"));

        let drained = queue.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].content, "first");
        assert_eq!(queue.len(), 1);

        let drained = queue.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].content, "second");
    }

    #[test]
    fn test_all_at_once() {
        let mut queue = MessageQueue::new().with_mode(QueueMode::AllAtOnce);

        queue.push(make_msg("first"));
        queue.push(make_msg("second"));

        let drained = queue.drain();
        assert_eq!(drained.len(), 2);
        assert!(queue.is_empty());
    }
}
