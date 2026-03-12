//! Message types and definitions.

use super::protocol::MessagePriority;
use serde::{Deserialize, Serialize};

/// A message to be sent between agents
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub message_type: MessageType,
    pub content: String,
    pub priority: MessagePriority,
    pub metadata: MessageMetadata,
}

impl Message {
    pub fn new(message_type: MessageType, content: impl Into<String>) -> Self {
        Self {
            message_type,
            content: content.into(),
            priority: MessagePriority::Normal,
            metadata: MessageMetadata::default(),
        }
    }

    pub fn with_priority(mut self, priority: MessagePriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_metadata(mut self, key: &str, value: impl Into<String>) -> Self {
        self.metadata.fields.insert(key.to_string(), value.into());
        self
    }
}

/// Types of messages
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum MessageType {
    /// Direct communication
    Direct,
    /// Task assignment
    Task,
    /// Task completion
    Completion,
    /// Coordination signal
    Coordination,
    /// Alert/Warning
    Alert,
    /// Information sharing
    Info,
    /// Query/Request
    Query,
    /// Response to query
    Response,
    /// Consensus proposal
    Proposal,
    /// Vote on proposal
    Vote,
    /// Stigmergic field signal
    StigmergicSignal,
    /// Discovery announcement
    Discovery,
    /// Heartbeat
    Heartbeat,
    /// Custom message type
    Custom(String),
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageType::Direct => write!(f, "direct"),
            MessageType::Task => write!(f, "task"),
            MessageType::Completion => write!(f, "completion"),
            MessageType::Coordination => write!(f, "coordination"),
            MessageType::Alert => write!(f, "alert"),
            MessageType::Info => write!(f, "info"),
            MessageType::Query => write!(f, "query"),
            MessageType::Response => write!(f, "response"),
            MessageType::Proposal => write!(f, "proposal"),
            MessageType::Vote => write!(f, "vote"),
            MessageType::StigmergicSignal => write!(f, "stigmergic"),
            MessageType::Discovery => write!(f, "discovery"),
            MessageType::Heartbeat => write!(f, "heartbeat"),
            MessageType::Custom(s) => write!(f, "custom:{}", s),
        }
    }
}

/// Message metadata
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct MessageMetadata {
    pub fields: std::collections::HashMap<String, String>,
}

impl MessageMetadata {
    pub fn new() -> Self {
        Self {
            fields: std::collections::HashMap::new(),
        }
    }

    pub fn with_field(mut self, key: &str, value: impl Into<String>) -> Self {
        self.fields.insert(key.to_string(), value.into());
        self
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.fields.get(key)
    }
}
