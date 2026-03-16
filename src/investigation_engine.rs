//! Investigation Engine - Core engine for recursive investigation workflows
//!
//! Features:
//! - Session persistence and lifecycle management
//! - Recursive sub-agent delegation
//! - Provider-agnostic LLM abstraction
//! - Evidence chain construction

use crate::agent_core::{now, AgentLoop, AgentState, Message, ModelConfig, Role};
use crate::investigation_tools::{InvestigationToolRegistry, ToolCallRecord};
use chrono;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

/// Unique identifier for investigation sessions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Investigation session with full state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationSession {
    pub id: SessionId,
    pub title: String,
    pub description: String,
    pub status: SessionStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub workspace: PathBuf,

    // Investigation state
    pub datasets: Vec<DatasetInfo>,
    pub entities: Vec<EntityInfo>,
    pub findings: Vec<FindingInfo>,
    pub evidence_chains: Vec<EvidenceChainInfo>,
    pub tool_calls: Vec<ToolCallRecord>,
    pub subtasks: Vec<SubtaskInfo>,

    // LLM conversation state
    pub conversation: Vec<Message>,

    // Configuration
    pub config: InvestigationConfig,

    // Metadata
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    Created,
    Active,
    Paused,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetInfo {
    pub id: String,
    pub name: String,
    pub source: String,
    pub record_count: usize,
    pub schema: HashMap<String, String>,
    pub loaded_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityInfo {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    pub aliases: Vec<String>,
    pub attributes: HashMap<String, serde_json::Value>,
    pub source_datasets: Vec<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingInfo {
    pub id: String,
    pub title: String,
    pub description: String,
    pub severity: FindingSeverity,
    pub confidence: f64,
    pub evidence_ids: Vec<String>,
    pub entity_ids: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceChainInfo {
    pub id: String,
    pub finding_id: String,
    pub description: String,
    pub steps: Vec<EvidenceStepInfo>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceStepInfo {
    pub step_number: usize,
    pub description: String,
    pub evidence_id: String,
    pub inference_rule: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtaskInfo {
    pub id: String,
    pub parent_id: Option<String>,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub status: SubtaskStatus,
    pub result: Option<String>,
    pub artifacts: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubtaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationConfig {
    pub max_subtask_depth: usize,
    pub max_concurrent_subtasks: usize,
    pub enable_recursive_delegation: bool,
    pub auto_save_interval_secs: u64,
    pub llm_config: ModelConfig,
}

impl Default for InvestigationConfig {
    fn default() -> Self {
        Self {
            max_subtask_depth: 3,
            max_concurrent_subtasks: 5,
            enable_recursive_delegation: true,
            auto_save_interval_secs: 60,
            llm_config: ModelConfig {
                provider: "ollama".to_string(),
                model: crate::ollama_client::resolve_model_from_env(
                    "hf.co/DavidAU/OpenAi-GPT-oss-20b-HERETIC-uncensored-NEO-Imatrix-gguf:IQ4_NL",
                ),
                api_url: "http://localhost:11434".to_string(),
                api_key: None,
                temperature: 0.7,
                max_tokens: 2000,
            },
        }
    }
}

impl InvestigationSession {
    pub fn new(
        title: impl Into<String>,
        description: impl Into<String>,
        workspace: PathBuf,
    ) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: SessionId::new(),
            title: title.into(),
            description: description.into(),
            status: SessionStatus::Created,
            created_at: now,
            updated_at: now,
            workspace,
            datasets: Vec::new(),
            entities: Vec::new(),
            findings: Vec::new(),
            evidence_chains: Vec::new(),
            tool_calls: Vec::new(),
            subtasks: Vec::new(),
            conversation: Vec::new(),
            config: InvestigationConfig::default(),
            metadata: HashMap::new(),
        }
    }

    pub fn add_dataset(&mut self, dataset: DatasetInfo) {
        self.datasets.push(dataset);
        self.updated_at = chrono::Utc::now();
    }

    pub fn add_entity(&mut self, entity: EntityInfo) {
        self.entities.push(entity);
        self.updated_at = chrono::Utc::now();
    }

    pub fn add_finding(&mut self, finding: FindingInfo) {
        self.findings.push(finding);
        self.updated_at = chrono::Utc::now();
    }

    pub fn add_subtask(&mut self, subtask: SubtaskInfo) {
        self.subtasks.push(subtask);
        self.updated_at = chrono::Utc::now();
    }

    pub fn update_subtask_status(&mut self, subtask_id: &str, status: SubtaskStatus) {
        if let Some(subtask) = self.subtasks.iter_mut().find(|s| s.id == subtask_id) {
            subtask.status = status;
            if status == SubtaskStatus::Completed || status == SubtaskStatus::Failed {
                subtask.completed_at = Some(chrono::Utc::now());
            }
            self.updated_at = chrono::Utc::now();
        }
    }

    pub fn add_tool_call(&mut self, call: ToolCallRecord) {
        self.tool_calls.push(call);
        self.updated_at = chrono::Utc::now();
    }

    pub fn add_message(&mut self, message: Message) {
        self.conversation.push(message);
        self.updated_at = chrono::Utc::now();
    }

    pub fn set_status(&mut self, status: SessionStatus) {
        self.status = status;
        self.updated_at = chrono::Utc::now();
    }

    /// Save session to disk
    pub async fn save(&self) -> anyhow::Result<()> {
        let path = self.workspace.join(format!("session_{}.json", self.id.0));
        let json = serde_json::to_string_pretty(self)?;
        tokio::fs::write(&path, json).await?;
        Ok(())
    }

    /// Load session from disk
    pub async fn load(workspace: &PathBuf, session_id: SessionId) -> anyhow::Result<Self> {
        let path = workspace.join(format!("session_{}.json", session_id.0));
        let json = tokio::fs::read_to_string(&path).await?;
        let session: Self = serde_json::from_str(&json)?;
        Ok(session)
    }

    /// List all saved sessions in workspace
    pub async fn list_sessions(workspace: &PathBuf) -> anyhow::Result<Vec<SessionSummary>> {
        let mut sessions = Vec::new();
        let mut entries = tokio::fs::read_dir(workspace).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(json) = tokio::fs::read_to_string(&path).await {
                    if let Ok(session) = serde_json::from_str::<InvestigationSession>(&json) {
                        sessions.push(SessionSummary {
                            id: session.id,
                            title: session.title,
                            description: session.description,
                            status: session.status,
                            created_at: session.created_at,
                            updated_at: session.updated_at,
                            finding_count: session.findings.len(),
                        });
                    }
                }
            }
        }

        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: SessionId,
    pub title: String,
    pub description: String,
    pub status: SessionStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub finding_count: usize,
}

/// Investigation Engine - orchestrates investigations
pub struct InvestigationEngine {
    session: RwLock<InvestigationSession>,
    #[allow(dead_code)]
    tool_registry: InvestigationToolRegistry,
    agent_loop: AgentLoop,
    event_sender: Option<mpsc::Sender<EngineEvent>>,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    SessionCreated(SessionId),
    SessionLoaded(SessionId),
    SessionSaved(SessionId),
    ToolCallStarted(String, serde_json::Value),
    ToolCallCompleted(String, Result<String, String>),
    SubtaskCreated(String, String), // id, description
    SubtaskCompleted(String, SubtaskStatus),
    FindingAdded(String),   // finding_id
    EntityResolved(String), // entity_id
    DatasetLoaded(String),  // dataset_id
    Error(String),
}

impl InvestigationEngine {
    pub fn new(session: InvestigationSession) -> Self {
        let workspace = session.workspace.clone();
        let tool_registry = InvestigationToolRegistry::new(workspace.clone());
        let agent_loop = AgentLoop::new(std::sync::Arc::new(RwLock::new(AgentState::new())))
            .with_tools(tool_registry.into_tools());

        Self {
            session: RwLock::new(session),
            tool_registry: InvestigationToolRegistry::new(workspace),
            agent_loop,
            event_sender: None,
        }
    }

    pub fn with_event_sender(mut self, sender: mpsc::Sender<EngineEvent>) -> Self {
        self.event_sender = Some(sender);
        self
    }

    fn emit(&self, event: EngineEvent) {
        if let Some(sender) = &self.event_sender {
            let _ = sender.try_send(event);
        }
    }

    /// Start a new investigation query
    pub async fn investigate(&self, query: &str) -> anyhow::Result<String> {
        // Set session active
        {
            let mut session = self.session.write().await;
            session.set_status(SessionStatus::Active);
            session.add_message(Message {
                role: Role::User,
                content: query.to_string(),
                tool_calls: None,
                tool_call_id: None,
                attachments: Vec::new(),
                timestamp: now(),
            });
        }

        // Run agent loop
        let user_message = Message {
            role: Role::User,
            content: query.to_string(),
            tool_calls: None,
            tool_call_id: None,
            attachments: Vec::new(),
            timestamp: chrono::Utc::now().timestamp() as u64,
        };

        let config = {
            let session = self.session.read().await;
            session.config.llm_config.clone()
        };

        match self.agent_loop.run(user_message, &config).await {
            Ok(_) => {
                // Get final response from conversation
                let session = self.session.read().await;
                let response = session
                    .conversation
                    .last()
                    .map(|m| m.content.clone())
                    .unwrap_or_else(|| "Investigation completed".to_string());

                self.emit(EngineEvent::SessionSaved(session.id));
                Ok(response)
            }
            Err(e) => {
                self.emit(EngineEvent::Error(e.to_string()));
                Err(anyhow::anyhow!("Investigation failed: {}", e))
            }
        }
    }

    /// Delegate a subtask for recursive investigation
    pub async fn delegate_subtask(
        &self,
        description: &str,
        acceptance_criteria: Vec<String>,
        parent_id: Option<String>,
    ) -> anyhow::Result<String> {
        let subtask_id = Uuid::new_v4().to_string();

        let subtask = SubtaskInfo {
            id: subtask_id.clone(),
            parent_id,
            description: description.to_string(),
            acceptance_criteria,
            status: SubtaskStatus::Pending,
            result: None,
            artifacts: Vec::new(),
            created_at: chrono::Utc::now(),
            completed_at: None,
        };

        {
            let mut session = self.session.write().await;
            session.add_subtask(subtask);
        }

        self.emit(EngineEvent::SubtaskCreated(
            subtask_id.clone(),
            description.to_string(),
        ));

        // In recursive mode, spawn a sub-investigation
        // For now, mark as completed with placeholder
        {
            let mut session = self.session.write().await;
            session.update_subtask_status(&subtask_id, SubtaskStatus::Completed);
        }

        self.emit(EngineEvent::SubtaskCompleted(
            subtask_id.clone(),
            SubtaskStatus::Completed,
        ));

        Ok(format!("Subtask {} completed", subtask_id))
    }

    /// Save current session state
    pub async fn save(&self) -> anyhow::Result<()> {
        let session = self.session.read().await;
        let id = session.id;
        session.save().await?;
        self.emit(EngineEvent::SessionSaved(id));
        Ok(())
    }

    /// Get session summary
    pub async fn get_summary(&self) -> InvestigationSummary {
        let session = self.session.read().await;
        InvestigationSummary {
            id: session.id,
            title: session.title.clone(),
            status: session.status,
            dataset_count: session.datasets.len(),
            entity_count: session.entities.len(),
            finding_count: session.findings.len(),
            subtask_count: session.subtasks.len(),
            completed_subtasks: session
                .subtasks
                .iter()
                .filter(|s| matches!(s.status, SubtaskStatus::Completed))
                .count(),
        }
    }

    /// Get full session state (for debugging/inspection)
    pub async fn get_session(&self) -> InvestigationSession {
        self.session.read().await.clone()
    }
}

#[derive(Debug, Clone)]
pub struct InvestigationSummary {
    pub id: SessionId,
    pub title: String,
    pub status: SessionStatus,
    pub dataset_count: usize,
    pub entity_count: usize,
    pub finding_count: usize,
    pub subtask_count: usize,
    pub completed_subtasks: usize,
}
