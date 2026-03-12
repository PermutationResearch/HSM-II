//! Storage backends for LCM
//!
//! Provides persistence for the DAG structure and nodes.

use super::{ContextDag, DagNode, NodeId};
use crate::database::RooDb;
use async_trait::async_trait;
use chrono::Utc;
use mysql_async::prelude::*;
use mysql_async::Value;
use regex::Regex;
use serde_json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Trait for LCM storage backends
#[async_trait]
pub trait LcmStorage: Send + Sync {
    /// Save a single node
    async fn save_node(&self, node_id: &NodeId, node: &DagNode) -> anyhow::Result<()>;

    /// Load a single node
    async fn load_node(&self, node_id: &NodeId) -> anyhow::Result<Option<DagNode>>;

    /// Save the entire DAG structure
    async fn save_dag(&self, dag: &ContextDag) -> anyhow::Result<()>;

    /// Load the entire DAG structure
    async fn load_dag(&self) -> anyhow::Result<Option<ContextDag>>;

    /// Search nodes by content pattern
    async fn search(&self, pattern: &str) -> anyhow::Result<Vec<(NodeId, DagNode)>>;

    /// Delete old nodes (for cleanup)
    async fn delete_older_than(&self, days: u32) -> anyhow::Result<usize>;
}

/// SQLite-backed storage for LCM
pub struct SqliteStorage {
    db_path: String,
}

impl SqliteStorage {
    fn nodes_path(&self) -> PathBuf {
        PathBuf::from(format!("{}.nodes.json", self.db_path))
    }

    fn dag_path(&self) -> PathBuf {
        PathBuf::from(format!("{}.dag.json", self.db_path))
    }

    async fn load_nodes_map(&self) -> anyhow::Result<HashMap<NodeId, DagNode>> {
        let p = self.nodes_path();
        match tokio::fs::read_to_string(&p).await {
            Ok(raw) => Ok(serde_json::from_str(&raw)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
            Err(e) => Err(e.into()),
        }
    }

    async fn save_nodes_map(&self, map: &HashMap<NodeId, DagNode>) -> anyhow::Result<()> {
        let p = self.nodes_path();
        if let Some(parent) = p.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let payload = serde_json::to_string_pretty(map)?;
        tokio::fs::write(p, payload).await?;
        Ok(())
    }
}

impl SqliteStorage {
    pub fn new(db_path: &str) -> Self {
        Self {
            db_path: db_path.to_string(),
        }
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.nodes_path().parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if tokio::fs::metadata(self.nodes_path()).await.is_err() {
            self.save_nodes_map(&HashMap::new()).await?;
        }
        if tokio::fs::metadata(self.dag_path()).await.is_err() {
            tokio::fs::write(self.dag_path(), "null").await?;
        }
        Ok(())
    }
}

#[async_trait]
impl LcmStorage for SqliteStorage {
    async fn save_node(&self, node_id: &NodeId, node: &DagNode) -> anyhow::Result<()> {
        let mut nodes = self.load_nodes_map().await?;
        nodes.insert(node_id.clone(), node.clone());
        self.save_nodes_map(&nodes).await?;
        Ok(())
    }

    async fn load_node(&self, node_id: &NodeId) -> anyhow::Result<Option<DagNode>> {
        let nodes = self.load_nodes_map().await?;
        Ok(nodes.get(node_id).cloned())
    }

    async fn save_dag(&self, dag: &ContextDag) -> anyhow::Result<()> {
        let p = self.dag_path();
        if let Some(parent) = p.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let payload = serde_json::to_string_pretty(dag)?;
        tokio::fs::write(p, payload).await?;
        Ok(())
    }

    async fn load_dag(&self) -> anyhow::Result<Option<ContextDag>> {
        match tokio::fs::read_to_string(self.dag_path()).await {
            Ok(raw) => Ok(serde_json::from_str::<Option<ContextDag>>(&raw)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn search(&self, pattern: &str) -> anyhow::Result<Vec<(NodeId, DagNode)>> {
        let regex = Regex::new(pattern).ok();
        let nodes = self.load_nodes_map().await?;
        let mut results = Vec::new();
        for (id, node) in nodes.iter() {
            let content = match &node.node_type {
                super::NodeType::Message { content, .. } => content.as_str(),
                super::NodeType::Summary { summary_text, .. } => summary_text.as_str(),
                super::NodeType::LargeFile {
                    exploration_summary,
                    ..
                } => exploration_summary.as_str(),
            };
            if let Some(ref re) = regex {
                if re.is_match(content) {
                    results.push((id.clone(), node.clone()));
                }
            }
        }
        Ok(results)
    }

    async fn delete_older_than(&self, days: u32) -> anyhow::Result<usize> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
        let mut nodes = self.load_nodes_map().await?;
        let before = nodes.len();
        nodes.retain(|_, node| node.created_at >= cutoff);
        let removed = before.saturating_sub(nodes.len());
        if removed > 0 {
            self.save_nodes_map(&nodes).await?;
        }
        Ok(removed)
    }
}

/// In-memory storage (for testing or ephemeral contexts)
pub struct MemoryStorage {
    nodes: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<NodeId, DagNode>>>,
    dag: std::sync::Arc<tokio::sync::RwLock<Option<ContextDag>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            nodes: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            dag: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LcmStorage for MemoryStorage {
    async fn save_node(&self, node_id: &NodeId, node: &DagNode) -> anyhow::Result<()> {
        let mut nodes = self.nodes.write().await;
        nodes.insert(node_id.clone(), node.clone());
        Ok(())
    }

    async fn load_node(&self, node_id: &NodeId) -> anyhow::Result<Option<DagNode>> {
        let nodes = self.nodes.read().await;
        Ok(nodes.get(node_id).cloned())
    }

    async fn save_dag(&self, dag: &ContextDag) -> anyhow::Result<()> {
        let mut d = self.dag.write().await;
        *d = Some(dag.clone());
        Ok(())
    }

    async fn load_dag(&self) -> anyhow::Result<Option<ContextDag>> {
        let d = self.dag.read().await;
        Ok(d.clone())
    }

    async fn search(&self, pattern: &str) -> anyhow::Result<Vec<(NodeId, DagNode)>> {
        let regex = Regex::new(pattern).ok();
        let nodes = self.nodes.read().await;
        let mut results = Vec::new();

        for (id, node) in nodes.iter() {
            let content = match &node.node_type {
                super::NodeType::Message { content, .. } => content.clone(),
                super::NodeType::Summary { summary_text, .. } => summary_text.clone(),
                super::NodeType::LargeFile {
                    exploration_summary,
                    ..
                } => exploration_summary.clone(),
            };

            if let Some(ref re) = regex {
                if re.is_match(&content) {
                    results.push((id.clone(), node.clone()));
                }
            }
        }

        Ok(results)
    }

    async fn delete_older_than(&self, _days: u32) -> anyhow::Result<usize> {
        // Memory storage doesn't persist, so nothing to delete
        Ok(0)
    }
}

/// RooDB-backed LCM storage.
pub struct RooStorage {
    db: Arc<RooDb>,
}

impl RooStorage {
    pub fn new(db: Arc<RooDb>) -> Self {
        Self { db }
    }

    pub async fn ensure_schema(&self) -> anyhow::Result<()> {
        let mut conn = self.db.get_conn().await?;
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS lcm_nodes (
                node_id VARCHAR(128) PRIMARY KEY,
                created_at BIGINT NOT NULL,
                node_data JSON NOT NULL
            )",
        )
        .await?;
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS lcm_dag (
                id TINYINT PRIMARY KEY,
                dag_data JSON NOT NULL
            )",
        )
        .await?;
        Ok(())
    }

    fn node_created_at(node: &DagNode) -> i64 {
        node.created_at.timestamp_millis()
    }
}

#[async_trait]
impl LcmStorage for RooStorage {
    async fn save_node(&self, node_id: &NodeId, node: &DagNode) -> anyhow::Result<()> {
        let mut conn = self.db.get_conn().await?;
        let payload = serde_json::to_string(node)?;
        let created_at = Self::node_created_at(node);
        conn.exec_drop(
            "INSERT INTO lcm_nodes (node_id, created_at, node_data) VALUES (?, ?, ?)
             ON DUPLICATE KEY UPDATE created_at = VALUES(created_at), node_data = VALUES(node_data)",
            (node_id, created_at, payload)
        ).await?;
        Ok(())
    }

    async fn load_node(&self, node_id: &NodeId) -> anyhow::Result<Option<DagNode>> {
        let mut conn = self.db.get_conn().await?;
        let row: Option<String> = conn
            .exec_first(
                "SELECT node_data FROM lcm_nodes WHERE node_id = ?",
                (node_id,),
            )
            .await?;
        if let Some(json) = row {
            let node: DagNode = serde_json::from_str(&json)?;
            Ok(Some(node))
        } else {
            Ok(None)
        }
    }

    async fn save_dag(&self, dag: &ContextDag) -> anyhow::Result<()> {
        let mut conn = self.db.get_conn().await?;
        let payload = serde_json::to_string(dag)?;
        conn.exec_drop(
            "INSERT INTO lcm_dag (id, dag_data) VALUES (1, ?)
             ON DUPLICATE KEY UPDATE dag_data = VALUES(dag_data)",
            (payload,),
        )
        .await?;
        Ok(())
    }

    async fn load_dag(&self) -> anyhow::Result<Option<ContextDag>> {
        let mut conn = self.db.get_conn().await?;
        let row: Option<String> = conn
            .exec_first("SELECT dag_data FROM lcm_dag WHERE id = 1", ())
            .await?;
        if let Some(json) = row {
            let dag: ContextDag = serde_json::from_str(&json)?;
            Ok(Some(dag))
        } else {
            Ok(None)
        }
    }

    async fn search(&self, pattern: &str) -> anyhow::Result<Vec<(NodeId, DagNode)>> {
        let mut conn = self.db.get_conn().await?;
        let regex = Regex::new(pattern).ok();
        let rows: Vec<Option<String>> = conn
            .query("SELECT node_data FROM lcm_nodes")
            .await?
            .into_iter()
            .map(mysql_value_to_string)
            .collect();
        let mut results = Vec::new();
        for json_opt in rows {
            if let Some(json) = json_opt {
                if let Ok(node) = serde_json::from_str::<DagNode>(&json) {
                    if regex.as_ref().map_or(true, |re| re.is_match(&json)) {
                        results.push((node.id.clone(), node));
                    }
                }
            }
        }
        Ok(results)
    }

    async fn delete_older_than(&self, days: u32) -> anyhow::Result<usize> {
        let mut conn = self.db.get_conn().await?;
        let cutoff = Utc::now()
            .checked_sub_signed(chrono::Duration::days(days as i64))
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0);
        conn.exec_drop("DELETE FROM lcm_nodes WHERE created_at < ?", (cutoff,))
            .await?;
        Ok(conn.affected_rows() as usize)
    }
}

fn mysql_value_to_string(value: Value) -> Option<String> {
    match value {
        Value::Bytes(bytes) => Some(String::from_utf8_lossy(&bytes).to_string()),
        _ => None,
    }
}
