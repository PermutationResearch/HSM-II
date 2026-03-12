//! Recursive Investigation Agent System
//! 
//! An autonomous agent for investigating heterogeneous datasets,
//! resolving entities, and surfacing non-obvious connections.
//! 
//! Features:
//! - 19 specialized tools for dataset analysis and investigation
//! - Recursive sub-agent delegation for parallel processing
//! - Provider-agnostic LLM abstraction
//! - Session persistence and lifecycle management
//! - Evidence-backed analysis with audit trails

pub mod config;
pub mod engine;
pub mod llm;
pub mod session;
pub mod tools;
pub mod datasets;
pub mod entity;
pub mod cli;
pub mod repl;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

/// Unique identifier for investigations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InvestigationId(pub Uuid);

impl InvestigationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for InvestigationId {
    fn default() -> Self {
        Self::new()
    }
}

/// Status of an investigation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvestigationStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Paused,
}

/// Core investigation structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Investigation {
    pub id: InvestigationId,
    pub title: String,
    pub description: String,
    pub status: InvestigationStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub datasets: Vec<DatasetRef>,
    pub entities: Vec<Entity>,
    pub findings: Vec<Finding>,
    pub evidence_chains: Vec<EvidenceChain>,
    pub workspace_path: PathBuf,
    pub metadata: HashMap<String, String>,
}

/// Reference to a dataset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetRef {
    pub id: String,
    pub name: String,
    pub source: DatasetSource,
    pub schema: DatasetSchema,
    pub entity_fields: Vec<String>,
    pub record_count: Option<usize>,
}

/// Source of a dataset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DatasetSource {
    File(PathBuf),
    Url(String),
    Database(String),
    Api(String),
}

/// Schema of a dataset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetSchema {
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    pub name: String,
    pub field_type: FieldType,
    pub nullable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FieldType {
    String,
    Integer,
    Float,
    Boolean,
    Date,
    DateTime,
    Json,
}

/// Entity resolved across datasets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub canonical_name: String,
    pub entity_type: EntityType,
    pub aliases: Vec<String>,
    pub attributes: HashMap<String, serde_json::Value>,
    pub source_records: Vec<SourceRecord>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EntityType {
    Person,
    Organization,
    Location,
    Contract,
    Campaign,
    LobbyingFiling,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRecord {
    pub dataset_id: String,
    pub record_id: String,
    pub raw_data: serde_json::Value,
}

/// Finding from investigation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub title: String,
    pub description: String,
    pub severity: FindingSeverity,
    pub confidence: f64,
    pub evidence: Vec<Evidence>,
    pub related_entities: Vec<String>,
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

/// Evidence item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub evidence_type: EvidenceType,
    pub description: String,
    pub source: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceType {
    Record,
    Analysis,
    Correlation,
    ExternalSource,
    Calculation,
}

/// Chain of evidence supporting a finding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceChain {
    pub id: String,
    pub finding_id: String,
    pub steps: Vec<EvidenceStep>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceStep {
    pub step_number: usize,
    pub description: String,
    pub evidence_id: String,
    pub inference_rule: String,
}

/// Tool call record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub result: ToolResult,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolResult {
    Success(serde_json::Value),
    Error(String),
    Partial(serde_json::Value, String),
}

/// Subtask for recursive delegation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtask {
    pub id: String,
    pub parent_id: Option<String>,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub status: SubtaskStatus,
    pub assigned_agent: Option<String>,
    pub result: Option<serde_json::Value>,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubtaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Blocked,
}

/// Artifact produced by subtask
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub name: String,
    pub artifact_type: ArtifactType,
    pub path: PathBuf,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactType {
    Json,
    Csv,
    Markdown,
    Html,
    Pdf,
    Image,
    Other,
}

impl Investigation {
    pub fn new(title: impl Into<String>, description: impl Into<String>, workspace: PathBuf) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: InvestigationId::new(),
            title: title.into(),
            description: description.into(),
            status: InvestigationStatus::Pending,
            created_at: now,
            updated_at: now,
            datasets: Vec::new(),
            entities: Vec::new(),
            findings: Vec::new(),
            evidence_chains: Vec::new(),
            workspace_path: workspace,
            metadata: HashMap::new(),
        }
    }

    pub fn add_dataset(&mut self, dataset: DatasetRef) {
        self.datasets.push(dataset);
        self.updated_at = chrono::Utc::now();
    }

    pub fn add_entity(&mut self, entity: Entity) {
        self.entities.push(entity);
        self.updated_at = chrono::Utc::now();
    }

    pub fn add_finding(&mut self, finding: Finding) {
        self.findings.push(finding);
        self.updated_at = chrono::Utc::now();
    }

    pub fn update_status(&mut self, status: InvestigationStatus) {
        self.status = status;
        self.updated_at = chrono::Utc::now();
    }
}
