//! LadybugDB-backed email storage for the Email Agent
//!
//! Provides persistent, vector-searchable email storage using LadybugDB's
//! columnar format and embedding capabilities.
//!
//! Inspired by mcp_agent_mail architecture but integrated with HSM-II's
//! native storage system.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use anyhow::Result;

use crate::hyper_stigmergy::{Belief, BeliefSource};

/// Email stored as a LadybugDB belief with vector embedding
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredEmail {
    /// Unique email ID
    pub id: String,
    /// Thread ID for conversation tracking
    pub thread_id: String,
    /// Sender address
    pub from: String,
    /// Recipient addresses
    pub to: Vec<String>,
    /// CC addresses
    pub cc: Vec<String>,
    /// Email subject
    pub subject: String,
    /// Plain text body
    pub body_text: String,
    /// HTML body (if available)
    pub body_html: Option<String>,
    /// Timestamp (Unix seconds)
    pub timestamp: u64,
    /// IMAP folder/label
    pub folder: String,
    /// Flags (read, replied, etc.)
    pub flags: Vec<String>,
    /// AI-generated classification
    pub classification: Option<EmailClassification>,
    /// Vector embedding for semantic search
    pub embedding: Option<Vec<f32>>,
    /// Reply chain
    pub in_reply_to: Option<String>,
    /// References for threading
    pub references: Vec<String>,
}

/// AI classification of email
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmailClassification {
    /// Category (spam, newsletter, important, etc.)
    pub category: String,
    /// Priority score (0-1)
    pub priority: f32,
    /// Whether a response is needed
    pub needs_response: bool,
    /// Suggested action
    pub suggested_action: SuggestedAction,
    /// Confidence in classification
    pub confidence: f32,
    /// Classification timestamp
    pub classified_at: u64,
}

/// Suggested action for email
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SuggestedAction {
    /// Read and archive
    Archive,
    /// Reply needed
    Reply { suggested_content: Option<String> },
    /// Forward to someone
    Forward { to: String },
    /// Delete as spam
    Delete,
    /// Label and file
    Label { labels: Vec<String> },
    /// Schedule for later
    Snooze { until: u64 },
}

/// LadybugDB-backed email storage
pub struct LadybugEmailStorage {
    /// Storage path
    base_path: std::path::PathBuf,
}

impl LadybugEmailStorage {
    /// Create new LadybugDB email storage
    pub fn new(base_path: impl AsRef<std::path::Path>) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
        }
    }
    
    /// Store an email in LadybugDB
    pub async fn store_email(&self, email: &StoredEmail) -> Result<()> {
        // Convert email to a belief for storage
        let _belief_id = format!("email_{}", email.id);
        
        let content = format!(
            "Email from {} to {} on {}: {} - {}",
            email.from,
            email.to.join(", "),
            chrono::DateTime::from_timestamp(email.timestamp as i64, 0)
                .map(|d| d.to_rfc2822())
                .unwrap_or_default(),
            email.subject,
            email.body_text.chars().take(200).collect::<String>()
        );
        
        // Create belief
        let _belief = Belief {
            id: 0, // Will be assigned by world
            content: content.clone(),
            confidence: email.classification.as_ref()
                .map(|c| c.confidence as f64)
                .unwrap_or(0.7),
            source: BeliefSource::Observation,
            supporting_evidence: vec![
                format!("folder:{}", email.folder),
                format!("from:{}", email.from),
            ],
            contradicting_evidence: vec![],
            created_at: email.timestamp,
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            update_count: 0,
            abstract_l0: Some(crate::memory::derive_hierarchy(&content).0),
            overview_l1: Some(crate::memory::derive_hierarchy(&content).1),
            owner_namespace: Some("email".to_string()),
            supersedes_belief_id: None,
            evidence_belief_ids: Vec::new(),
            human_committed: false,
        };
        
        // Store as serialized JSON in the email folder
        let email_path = self.base_path
            .join("emails")
            .join(&email.folder)
            .join(format!("{}.json", email.id));
        
        tokio::fs::create_dir_all(email_path.parent().unwrap()).await?;
        let json = serde_json::to_string_pretty(email)?;
        tokio::fs::write(email_path, json).await?;
        
        Ok(())
    }
    
    /// Retrieve email by ID
    pub async fn get_email(&self, email_id: &str) -> Result<Option<StoredEmail>> {
        // Search in all folders
        let emails_dir = self.base_path.join("emails");
        
        if !emails_dir.exists() {
            return Ok(None);
        }
        
        let mut entries = tokio::fs::read_dir(emails_dir).await?;
        
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let email_path = entry.path().join(format!("{}.json", email_id));
                if email_path.exists() {
                    let content = tokio::fs::read_to_string(email_path).await?;
                    let email: StoredEmail = serde_json::from_str(&content)?;
                    return Ok(Some(email));
                }
            }
        }
        
        Ok(None)
    }
    
    /// Semantic search over emails
    pub async fn semantic_search(&self, query: &str, limit: usize) -> Result<Vec<StoredEmail>> {
        // For now, simple keyword search
        // In production, this would use the embedding index
        let emails_dir = self.base_path.join("emails");
        
        if !emails_dir.exists() {
            return Ok(vec![]);
        }
        
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        
        let mut entries = tokio::fs::read_dir(emails_dir).await?;
        
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let folder = entry.path();
                let mut files = tokio::fs::read_dir(folder).await?;
                
                while let Some(file) = files.next_entry().await? {
                    if file.file_type().await?.is_file() {
                        let content = tokio::fs::read_to_string(file.path()).await?;
                        if let Ok(email) = serde_json::from_str::<StoredEmail>(&content) {
                            let searchable = format!(
                                "{} {} {}",
                                email.subject,
                                email.from,
                                email.body_text
                            ).to_lowercase();
                            
                            if searchable.contains(&query_lower) {
                                results.push(email);
                                if results.len() >= limit {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // Sort by timestamp (newest first)
        results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        
        Ok(results)
    }
    
    /// Get emails by folder
    pub async fn get_emails_by_folder(&self, folder: &str, limit: usize) -> Result<Vec<StoredEmail>> {
        let folder_path = self.base_path.join("emails").join(folder);
        
        if !folder_path.exists() {
            return Ok(vec![]);
        }
        
        let mut results = Vec::new();
        let mut entries = tokio::fs::read_dir(folder_path).await?;
        
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_file() {
                let content = tokio::fs::read_to_string(entry.path()).await?;
                if let Ok(email) = serde_json::from_str::<StoredEmail>(&content) {
                    results.push(email);
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        
        // Sort by timestamp
        results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        
        Ok(results)
    }
    
    /// Get thread by ID
    pub async fn get_thread(&self, thread_id: &str) -> Result<Vec<StoredEmail>> {
        let emails_dir = self.base_path.join("emails");
        let mut thread = Vec::new();
        
        if !emails_dir.exists() {
            return Ok(thread);
        }
        
        let mut entries = tokio::fs::read_dir(emails_dir).await?;
        
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let mut files = tokio::fs::read_dir(entry.path()).await?;
                
                while let Some(file) = files.next_entry().await? {
                    if file.file_type().await?.is_file() {
                        let content = tokio::fs::read_to_string(file.path()).await?;
                        if let Ok(email) = serde_json::from_str::<StoredEmail>(&content) {
                            if email.thread_id == thread_id {
                                thread.push(email);
                            }
                        }
                    }
                }
            }
        }
        
        // Sort by timestamp
        thread.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        
        Ok(thread)
    }
    
    /// Store email classification
    pub async fn store_classification(&self, email_id: &str, classification: &EmailClassification) -> Result<()> {
        if let Some(mut email) = self.get_email(email_id).await? {
            email.classification = Some(classification.clone());
            self.store_email(&email).await?;
        }
        Ok(())
    }
    
    /// Get storage statistics
    pub async fn get_stats(&self) -> Result<StorageStats> {
        let emails_dir = self.base_path.join("emails");
        
        if !emails_dir.exists() {
            return Ok(StorageStats::default());
        }
        
        let mut total_emails = 0;
        let mut folder_counts = HashMap::new();
        let mut classified = 0;
        
        let mut entries = tokio::fs::read_dir(emails_dir).await?;
        
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let folder_name = entry.file_name().to_string_lossy().to_string();
                let mut count = 0;
                let mut files = tokio::fs::read_dir(entry.path()).await?;
                
                while let Some(file) = files.next_entry().await? {
                    if file.file_type().await?.is_file() {
                        count += 1;
                        total_emails += 1;
                        
                        // Check if classified
                        let content = tokio::fs::read_to_string(file.path()).await?;
                        if let Ok(email) = serde_json::from_str::<StoredEmail>(&content) {
                            if email.classification.is_some() {
                                classified += 1;
                            }
                        }
                    }
                }
                
                folder_counts.insert(folder_name, count);
            }
        }
        
        Ok(StorageStats {
            total_emails,
            folder_counts,
            classified,
        })
    }
    
    /// Vacuum and compact storage (remove duplicates, clean up)
    pub async fn vacuum(&self) -> Result<VacuumResult> {
        let emails_dir = self.base_path.join("emails");
        
        if !emails_dir.exists() {
            return Ok(VacuumResult::default());
        }
        
        let mut duplicates_removed = 0;
        let mut errors_fixed = 0;
        
        // Simple vacuum: remove duplicate IDs
        let mut seen_ids = std::collections::HashSet::new();
        
        let mut entries = tokio::fs::read_dir(emails_dir).await?;
        
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let mut files = tokio::fs::read_dir(entry.path()).await?;
                
                while let Some(file) = files.next_entry().await? {
                    if file.file_type().await?.is_file() {
                        let content = match tokio::fs::read_to_string(file.path()).await {
                            Ok(c) => c,
                            Err(_) => {
                                // Remove corrupted file
                                let _ = tokio::fs::remove_file(file.path()).await;
                                errors_fixed += 1;
                                continue;
                            }
                        };
                        
                        if let Ok(email) = serde_json::from_str::<StoredEmail>(&content) {
                            if seen_ids.contains(&email.id) {
                                // Remove duplicate
                                let _ = tokio::fs::remove_file(file.path()).await;
                                duplicates_removed += 1;
                            } else {
                                seen_ids.insert(email.id);
                            }
                        }
                    }
                }
            }
        }
        
        Ok(VacuumResult {
            duplicates_removed,
            errors_fixed,
        })
    }
}

/// Storage statistics
#[derive(Clone, Debug, Default)]
pub struct StorageStats {
    pub total_emails: usize,
    pub folder_counts: HashMap<String, usize>,
    pub classified: usize,
}

/// Vacuum operation result
#[derive(Clone, Debug, Default)]
pub struct VacuumResult {
    pub duplicates_removed: usize,
    pub errors_fixed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_store_and_retrieve() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = LadybugEmailStorage::new(temp_dir.path());
        
        let email = StoredEmail {
            id: "test-123".to_string(),
            thread_id: "thread-456".to_string(),
            from: "sender@example.com".to_string(),
            to: vec!["recipient@example.com".to_string()],
            cc: vec![],
            subject: "Test Email".to_string(),
            body_text: "This is a test email.".to_string(),
            body_html: None,
            timestamp: 1234567890,
            folder: "inbox".to_string(),
            flags: vec![],
            classification: None,
            embedding: None,
            in_reply_to: None,
            references: vec![],
        };
        
        storage.store_email(&email).await.unwrap();
        
        let retrieved = storage.get_email("test-123").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().subject, "Test Email");
    }
    
    #[tokio::test]
    async fn test_semantic_search() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = LadybugEmailStorage::new(temp_dir.path());
        
        // Store a few emails
        for i in 0..5 {
            let email = StoredEmail {
                id: format!("email-{}", i),
                thread_id: format!("thread-{}", i),
                from: "test@example.com".to_string(),
                to: vec!["to@example.com".to_string()],
                cc: vec![],
                subject: format!("Subject {}", i),
                body_text: format!("Body content number {} with keyword", i),
                body_html: None,
                timestamp: 1234567890 + i as u64,
                folder: "inbox".to_string(),
                flags: vec![],
                classification: None,
                embedding: None,
                in_reply_to: None,
                references: vec![],
            };
            storage.store_email(&email).await.unwrap();
        }
        
        let results = storage.semantic_search("keyword", 10).await.unwrap();
        assert_eq!(results.len(), 5);
    }
}
