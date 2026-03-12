//! Core types for optimize_anything

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// An artifact being optimized (code, prompt, config, etc.)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Artifact {
    pub content: String,
    pub metadata: HashMap<String, String>,
    pub id: String,
}

impl Artifact {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            metadata: HashMap::new(),
            id: format!("artifact_{}", uuid::Uuid::new_v4()),
        }
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Actionable Side Information - diagnostic feedback for the proposer
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ASI {
    pub text: Vec<String>,
    pub structured: HashMap<String, String>,
    pub scores: HashMap<String, f64>,
}

impl ASI {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn log(mut self, message: impl Into<String>) -> Self {
        self.text.push(message.into());
        self
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.structured.insert(key.into(), value.into());
        self
    }

    pub fn with_score(mut self, key: impl Into<String>, value: f64) -> Self {
        self.scores.insert(key.into(), value);
        self
    }
}

/// A candidate solution in the search
#[derive(Clone, Debug)]
pub struct Candidate {
    pub artifact: Artifact,
    pub score: f64,
    pub asi: ASI,
    pub generation: usize,
    pub parents: Vec<String>,
}

impl Candidate {
    pub fn new(artifact: Artifact, score: f64, asi: ASI, generation: usize) -> Self {
        Self {
            artifact,
            score,
            asi,
            generation,
            parents: vec![],
        }
    }

    pub fn with_parents(mut self, parents: Vec<String>) -> Self {
        self.parents = parents;
        self
    }
}

/// Optimization mode
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum OptimizationMode {
    SingleTask,
    MultiTask,
    Generalization,
}

impl std::fmt::Display for OptimizationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OptimizationMode::SingleTask => write!(f, "single-task"),
            OptimizationMode::MultiTask => write!(f, "multi-task"),
            OptimizationMode::Generalization => write!(f, "generalization"),
        }
    }
}

impl Default for OptimizationMode {
    fn default() -> Self {
        OptimizationMode::SingleTask
    }
}
