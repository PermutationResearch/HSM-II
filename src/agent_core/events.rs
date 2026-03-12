//! Agent Events - Event-driven architecture for reactive UIs

use super::{Message, ToolCall};
use std::sync::Mutex;

/// Events emitted by the agent loop
#[derive(Clone, Debug)]
pub enum AgentEvent {
    // Connection events
    Connected,
    Disconnected,
    Error(String),

    // Message events
    MessageStart,
    MessageDelta {
        content: String,
    },
    MessageComplete(Message),

    // Tool events
    ToolStart {
        tool_call: ToolCall,
    },
    ToolExecuting {
        name: String,
        args: serde_json::Value,
    },
    ToolComplete {
        tool_call_id: String,
        result: String,
    },
    ToolError {
        tool_call_id: String,
        error: String,
    },

    // Thinking events (for models like DeepSeek-R1)
    ThinkingStart,
    ThinkingDelta {
        content: String,
    },
    ThinkingComplete(String),

    // Turn events
    TurnStart {
        turn: u32,
    },
    TurnComplete {
        turn: u32,
    },

    // State events
    StateChanged,
    ConversationCleared,

    // Queue events
    QueuedMessagesInjected {
        count: usize,
    },

    // Completion
    Done,
}

/// Event handler trait
pub trait EventHandler: Send + Sync {
    fn on_event(&self, event: &AgentEvent);
}

impl<F> EventHandler for F
where
    F: Fn(&AgentEvent) + Send + Sync,
{
    fn on_event(&self, event: &AgentEvent) {
        (self)(event);
    }
}

/// Event bus for subscribing to agent events
pub struct EventBus {
    handlers: Mutex<Vec<Box<dyn EventHandler>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            handlers: Mutex::new(Vec::new()),
        }
    }

    pub fn subscribe<H>(&self, handler: H)
    where
        H: EventHandler + 'static,
    {
        self.handlers.lock().unwrap().push(Box::new(handler));
    }

    pub fn emit(&self, event: &AgentEvent) {
        let handlers = self.handlers.lock().unwrap();
        for handler in handlers.iter() {
            handler.on_event(event);
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_handler() {
        let bus = EventBus::new();
        let received = std::sync::Arc::new(std::sync::Mutex::new(false));

        let r = received.clone();
        bus.subscribe(move |event: &AgentEvent| {
            if matches!(event, AgentEvent::Connected) {
                *r.lock().unwrap() = true;
            }
        });

        bus.emit(&AgentEvent::Connected);

        assert!(*received.lock().unwrap());
    }
}
