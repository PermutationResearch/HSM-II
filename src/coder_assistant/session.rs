//! Session Management for Coder Assistant
//!
//! Handles session persistence, loading, and event tracking

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Session metadata
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub messages: Vec<Message>,
    pub provider_config: ProviderConfig,
    pub context_files: Vec<String>,
    pub metadata: HashMap<String, String>,
}

impl Session {
    pub fn new(name: impl Into<String>) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let now = super::now();

        Self {
            id,
            name: name.into(),
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
            provider_config: ProviderConfig::default(),
            context_files: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    pub fn with_provider(mut self, config: ProviderConfig) -> Self {
        self.provider_config = config;
        self
    }

    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
        self.updated_at = super::now();
    }

    pub fn add_context_file(&mut self, path: impl Into<String>) {
        self.context_files.push(path.into());
        self.updated_at = super::now();
    }

    pub fn set_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.metadata.insert(key.into(), value.into());
        self.updated_at = super::now();
    }

    /// Get last n messages
    pub fn last_messages(&self, n: usize) -> &[Message] {
        let start = self.messages.len().saturating_sub(n);
        &self.messages[start..]
    }

    /// Clear messages except system
    pub fn clear(&mut self) {
        let system_msgs: Vec<Message> = self
            .messages
            .iter()
            .filter(|m| m.role == MessageRole::System)
            .cloned()
            .collect();
        self.messages = system_msgs;
        self.updated_at = super::now();
    }

    /// Export to JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Import from JSON
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Session event for event tracking
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SessionEvent {
    SessionCreated {
        session_id: String,
        name: String,
    },
    MessageAdded {
        session_id: String,
        message_index: usize,
    },
    ContextFileAdded {
        session_id: String,
        path: String,
    },
    SessionSaved {
        session_id: String,
        path: String,
    },
    SessionLoaded {
        session_id: String,
        path: String,
    },
    SessionCleared {
        session_id: String,
    },
}

/// Session manager
pub struct SessionManager {
    sessions: HashMap<String, Session>,
    current_session_id: Option<String>,
    storage_dir: PathBuf,
    event_listeners: Vec<Box<dyn Fn(&SessionEvent) + Send + Sync>>,
}

impl SessionManager {
    pub fn new(storage_dir: impl AsRef<Path>) -> Self {
        let storage_dir = storage_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&storage_dir).ok();

        Self {
            sessions: HashMap::new(),
            current_session_id: None,
            storage_dir,
            event_listeners: Vec::new(),
        }
    }

    /// Create a new session
    pub fn create_session(&mut self, name: impl Into<String>) -> &mut Session {
        let session = Session::new(name);
        let id = session.id.clone();

        self.emit_event(SessionEvent::SessionCreated {
            session_id: id.clone(),
            name: session.name.clone(),
        });

        self.sessions.insert(id.clone(), session);
        self.current_session_id = Some(id.clone());
        self.sessions.get_mut(&id).unwrap()
    }

    /// Get current session
    pub fn current_session(&self) -> Option<&Session> {
        self.current_session_id
            .as_ref()
            .and_then(|id| self.sessions.get(id))
    }

    /// Get current session (mutable)
    pub fn current_session_mut(&mut self) -> Option<&mut Session> {
        self.current_session_id
            .as_ref()
            .and_then(|id| self.sessions.get_mut(id))
    }

    /// Switch to a different session
    pub fn switch_session(&mut self, session_id: &str) -> bool {
        if self.sessions.contains_key(session_id) {
            self.current_session_id = Some(session_id.to_string());
            true
        } else {
            false
        }
    }

    /// Get session by ID
    pub fn get(&self, session_id: &str) -> Option<&Session> {
        self.sessions.get(session_id)
    }

    /// Get session by ID (mutable)
    pub fn get_mut(&mut self, session_id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(session_id)
    }

    /// List all sessions
    pub fn list(&self) -> Vec<&Session> {
        self.sessions.values().collect()
    }

    /// Delete a session
    pub fn delete(&mut self, session_id: &str) -> bool {
        if let Some(_) = self.sessions.remove(session_id) {
            // Delete from disk too
            let path = self.session_file_path(session_id);
            std::fs::remove_file(path).ok();

            if self.current_session_id.as_deref() == Some(session_id) {
                self.current_session_id = None;
            }
            true
        } else {
            false
        }
    }

    /// Save session to disk
    pub fn save(&self, session_id: &str) -> Result<(), SessionError> {
        let session = self
            .sessions
            .get(session_id)
            .ok_or(SessionError::SessionNotFound)?;

        let path = self.session_file_path(session_id);
        let json = session
            .to_json()
            .map_err(|e| SessionError::SerializationError(e.to_string()))?;

        std::fs::write(&path, json).map_err(|e| SessionError::IoError(e.to_string()))?;

        self.emit_event(SessionEvent::SessionSaved {
            session_id: session_id.to_string(),
            path: path.to_string_lossy().to_string(),
        });

        Ok(())
    }

    /// Load session from disk
    pub fn load(&mut self, session_id: &str) -> Result<&mut Session, SessionError> {
        let path = self.session_file_path(session_id);
        let json =
            std::fs::read_to_string(&path).map_err(|e| SessionError::IoError(e.to_string()))?;

        let session = Session::from_json(&json)
            .map_err(|e| SessionError::SerializationError(e.to_string()))?;

        let id = session.id.clone();
        self.sessions.insert(id.clone(), session);

        self.emit_event(SessionEvent::SessionLoaded {
            session_id: id.clone(),
            path: path.to_string_lossy().to_string(),
        });

        Ok(self.sessions.get_mut(&id).unwrap())
    }

    /// Load all sessions from disk
    pub fn load_all(&mut self) -> Result<Vec<&Session>, SessionError> {
        let session_ids: Vec<String> = std::fs::read_dir(&self.storage_dir)
            .map_err(|e| SessionError::IoError(e.to_string()))?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "json")
                    .unwrap_or(false)
            })
            .filter_map(|e| {
                e.path()
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
            })
            .collect();

        for session_id in session_ids {
            let _ = self.load(&session_id);
        }

        Ok(self.sessions.values().collect())
    }

    /// Save all sessions
    pub fn save_all(&self) -> Result<(), SessionError> {
        for session_id in self.sessions.keys() {
            self.save(session_id)?;
        }
        Ok(())
    }

    /// Add event listener
    pub fn on_event<F>(&mut self, callback: F)
    where
        F: Fn(&SessionEvent) + Send + Sync + 'static,
    {
        self.event_listeners.push(Box::new(callback));
    }

    fn emit_event(&self, event: SessionEvent) {
        for listener in &self.event_listeners {
            listener(&event);
        }
    }

    fn session_file_path(&self, session_id: &str) -> PathBuf {
        self.storage_dir.join(format!("{}.json", session_id))
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new("./sessions")
    }
}

/// Session error types
#[derive(Debug, Clone)]
pub enum SessionError {
    SessionNotFound,
    SerializationError(String),
    IoError(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::SessionNotFound => write!(f, "Session not found"),
            SessionError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            SessionError::IoError(msg) => write!(f, "IO error: {}", msg),
        }
    }
}

impl std::error::Error for SessionError {}

/// Project context file
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectContext {
    pub name: String,
    pub description: String,
    pub root_dir: String,
    pub include_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub file_summaries: HashMap<String, String>,
}

impl ProjectContext {
    pub fn new(name: impl Into<String>, root_dir: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            root_dir: root_dir.into(),
            include_patterns: vec!["*.rs".to_string(), "*.md".to_string()],
            exclude_patterns: vec!["target/".to_string(), ".git/".to_string()],
            file_summaries: HashMap::new(),
        }
    }

    pub fn load_from_dir(dir: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let dir = dir.as_ref();
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();

        Ok(Self::new(name, dir.to_string_lossy().to_string()))
    }

    pub fn build_index(&mut self) -> Result<(), std::io::Error> {
        self.file_summaries.clear();
        self.scan_directory(&self.root_dir.clone())
    }

    fn scan_directory(&mut self, dir: &str) -> Result<(), std::io::Error> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let path_str = path.to_string_lossy().to_string();

            // Check exclude patterns
            if self.exclude_patterns.iter().any(|p| path_str.contains(p)) {
                continue;
            }

            if path.is_dir() {
                self.scan_directory(&path_str)?;
            } else if path.is_file() {
                // Check include patterns
                if self.include_patterns.iter().any(|p| {
                    if path.extension().is_some() {
                        path_str.ends_with(&p[1..]) // Remove *
                    } else {
                        false
                    }
                }) {
                    // Generate simple summary
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let lines: Vec<&str> = content.lines().take(10).collect();
                        let summary = lines.join("\n");
                        self.file_summaries.insert(path_str, summary);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn to_prompt(&self) -> String {
        let mut prompt = format!("Project: {}\n", self.name);
        prompt.push_str(&format!("Description: {}\n", self.description));
        prompt.push_str(&format!("Root: {}\n\n", self.root_dir));

        prompt.push_str("Key files:\n");
        for (path, summary) in &self.file_summaries {
            prompt.push_str(&format!(
                "- {}: {}\n",
                path,
                if summary.len() > 100 {
                    &summary[..100]
                } else {
                    summary
                }
            ));
        }

        prompt
    }
}
