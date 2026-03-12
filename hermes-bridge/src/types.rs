//! Type definitions for Hermes Bridge

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Request to execute a task via Hermes
#[derive(Clone, Debug, Serialize)]
pub struct ExecutionRequest {
    /// Unique task identifier (generated if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    
    /// The prompt/task to execute
    pub prompt: String,
    
    /// Toolsets to enable for this execution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toolsets: Option<Vec<String>>,
    
    /// Maximum turns (tool calls) allowed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    
    /// HSM-II context to pass to Hermes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<HSMIIContext>,
    
    /// System prompt override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
}

/// HSM-II context passed to Hermes
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HSMIIContext {
    /// Memory state from HSM-II
    pub memory: HashMap<String, String>,
    
    /// User profile information
    pub user_profile: UserProfile,
    
    /// Serialized HSM-II state
    pub hsmii_state: serde_json::Value,
}

/// User profile for Hermes
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserProfile {
    pub name: String,
    pub expertise: Vec<String>,
    pub preferences: HashMap<String, String>,
}

/// Response from Hermes execution
#[derive(Clone, Debug, Deserialize)]
pub struct ExecutionResponse {
    pub task_id: String,
    pub result: String,
    pub tool_calls: Vec<ToolCall>,
    pub trajectory: Vec<Turn>,
    pub status: ExecutionStatus,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

/// Execution status
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub enum ExecutionStatus {
    Success,
    PartialSuccess,
    Failed,
    Timeout,
    Cancelled,
}

impl From<String> for ExecutionStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "success" => ExecutionStatus::Success,
            "partial_success" => ExecutionStatus::PartialSuccess,
            "failed" => ExecutionStatus::Failed,
            "timeout" => ExecutionStatus::Timeout,
            "cancelled" => ExecutionStatus::Cancelled,
            _ => ExecutionStatus::Failed,
        }
    }
}

/// A tool call made by Hermes
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
    pub result: Option<ToolResult>,
}

/// Result of a tool execution
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

/// A turn in the conversation trajectory
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Turn {
    pub turn_number: u32,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

/// Hermes context (internal API)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HermesContext {
    pub memory: HashMap<String, String>,
    pub user_profile: UserProfile,
    pub hsmii_state: serde_json::Value,
}

/// Health status from Hermes
#[derive(Clone, Debug, Deserialize)]
pub struct HermesHealth {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
    pub available_toolsets: Vec<String>,
    pub active_sessions: u32,
}

/// Skill definition for Hermes
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HermesSkill {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Result of skill synchronization
#[derive(Clone, Debug, Deserialize)]
pub struct SkillSyncResult {
    pub imported: Vec<HermesSkill>,
    pub exported: Vec<HermesSkill>,
    pub conflicts: Vec<SkillConflict>,
}

/// Skill conflict during sync
#[derive(Clone, Debug, Deserialize)]
pub struct SkillConflict {
    pub skill_name: String,
    pub reason: String,
    pub hermes_version: Option<HermesSkill>,
    pub hsmii_version: Option<HermesSkill>,
}

/// Federation message from HSM-II to Hermes Gateway
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FederationMessage {
    pub message_id: String,
    pub source_node: String,
    pub target_gateway: String,
    pub signal: StigmergicSignal,
    pub timestamp: u64,
}

/// Stigmergic signal representation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StigmergicSignal {
    pub signal_type: SignalType,
    pub payload: serde_json::Value,
    pub coherence: f64,
    pub origin: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SignalType {
    BeliefUpdate,
    ExperienceShare,
    SkillProposal,
    CoordinationRequest,
    Alert,
}

/// CASS Skill for conversion
#[derive(Clone, Debug)]
pub struct CASSSkill {
    pub id: String,
    pub title: String,
    pub principle: String,
    pub level: SkillLevel,
    pub confidence: f64,
    pub embedding: Option<Vec<f32>>,
}

#[derive(Clone, Debug)]
pub enum SkillLevel {
    General,
    RoleSpecific(String),
    TaskSpecific(String),
}

/// Builder for ExecutionRequest
pub struct ExecutionRequestBuilder {
    request: ExecutionRequest,
}

impl ExecutionRequestBuilder {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            request: ExecutionRequest {
                task_id: None,
                prompt: prompt.into(),
                toolsets: None,
                max_turns: None,
                context: None,
                system_prompt: None,
            },
        }
    }

    pub fn task_id(mut self, id: impl Into<String>) -> Self {
        self.request.task_id = Some(id.into());
        self
    }

    pub fn toolsets(mut self, toolsets: Vec<String>) -> Self {
        self.request.toolsets = Some(toolsets);
        self
    }

    pub fn max_turns(mut self, turns: u32) -> Self {
        self.request.max_turns = Some(turns);
        self
    }

    pub fn context(mut self, context: HSMIIContext) -> Self {
        self.request.context = Some(context);
        self
    }

    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.request.system_prompt = Some(prompt.into());
        self
    }

    pub fn build(self) -> ExecutionRequest {
        self.request
    }
}

impl ExecutionRequest {
    pub fn builder(prompt: impl Into<String>) -> ExecutionRequestBuilder {
        ExecutionRequestBuilder::new(prompt)
    }
}
