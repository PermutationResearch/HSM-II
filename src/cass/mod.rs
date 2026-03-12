//! Context-Aware Semantic Skills (CASS) - Semantic skill registry with embedding-based retrieval.
//!
//! CASS extends the existing skill system with:
//! - Semantic skill embeddings for natural language matching
//! - Context-aware skill ranking based on current situation
//! - Skill composition and chaining based on semantic similarity
//! - Integration with existing SkillBank for persistence
//!
//! Works with RooDB for skill storage and LARS for skill cascade triggers.

use crate::skill::{Skill, SkillBank};
use std::collections::HashMap;

pub mod context;
pub mod embedding;
pub mod semantic_graph;

pub use context::{ContextManager, ContextSnapshot, RelevanceScorer};
pub use embedding::{EmbeddingEngine, SimilarityMetric, SkillEmbedding};
pub use semantic_graph::{EdgeType, SemanticGraph, SkillNode};

/// CASS-enhanced skill registry
pub struct CASS {
    /// Underlying skill bank
    skill_bank: SkillBank,
    /// Embedding engine for semantic matching
    embedding_engine: EmbeddingEngine,
    /// Context manager for situation-aware retrieval
    context_manager: ContextManager,
    /// Semantic graph of skill relationships
    semantic_graph: SemanticGraph,
    /// In-memory embedding cache
    embedding_cache: HashMap<String, SkillEmbedding>,
}

impl CASS {
    pub fn new(skill_bank: SkillBank) -> Self {
        Self {
            skill_bank,
            embedding_engine: EmbeddingEngine::new(),
            context_manager: ContextManager::new(),
            semantic_graph: SemanticGraph::new(),
            embedding_cache: HashMap::new(),
        }
    }

    /// Initialize CASS from existing skills
    pub async fn initialize(&mut self) -> anyhow::Result<()> {
        // Build embeddings for all existing skills
        for skill in self.skill_bank.all_skills() {
            let embedding = self.embedding_engine.embed_skill(skill).await?;
            self.embedding_cache
                .insert(skill.id.clone(), embedding.clone());
            self.semantic_graph.add_skill_node(skill, embedding);
        }

        // Build semantic relationships
        self.semantic_graph
            .build_relationships(&self.embedding_cache);

        Ok(())
    }

    /// Semantic skill search with natural language query
    pub async fn search(
        &self,
        query: &str,
        context: Option<ContextSnapshot>,
        top_k: usize,
    ) -> Vec<SemanticSkillMatch> {
        // Embed query
        let query_embedding = self
            .embedding_engine
            .embed_text(query)
            .await
            .unwrap_or_default();

        // Score all skills
        let mut scored: Vec<(String, f64)> = Vec::new();

        for (skill_id, skill_embedding) in &self.embedding_cache {
            // Semantic similarity
            let semantic_score =
                skill_embedding.similarity(&query_embedding, SimilarityMetric::Cosine);

            // Context relevance (if context provided)
            let context_score = if let Some(ref ctx) = context {
                self.context_manager.relevance_score(skill_id, ctx)
            } else {
                0.5 // Neutral if no context
            };

            // Graph proximity (skills related to previously successful ones)
            let graph_score = self.semantic_graph.centrality_score(skill_id);

            // Combined score
            let combined = semantic_score * 0.6 + context_score * 0.3 + graph_score * 0.1;
            scored.push((skill_id.clone(), combined));
        }

        // Sort by score and take top_k
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        scored
            .into_iter()
            .take(top_k)
            .filter_map(|(id, score)| {
                self.skill_bank
                    .get_skill(&id)
                    .map(|skill| SemanticSkillMatch {
                        skill: skill.clone(),
                        semantic_score: score,
                        context_relevance: context
                            .as_ref()
                            .map(|ctx| self.context_manager.relevance_score(&id, ctx))
                            .unwrap_or(0.5),
                        related_skills: self.semantic_graph.related_skills(&id, 3),
                    })
            })
            .collect()
    }

    /// Add a new skill with automatic embedding and relationship building
    pub async fn add_skill(&mut self, skill: Skill) -> anyhow::Result<()> {
        use crate::skill::SkillLevel;

        // Generate embedding
        let embedding = self.embedding_engine.embed_skill(&skill).await?;

        // Store in appropriate collection based on level
        match skill.level {
            SkillLevel::General => self.skill_bank.general_skills.push(skill.clone()),
            SkillLevel::RoleSpecific(role) => {
                let role_key = format!("{:?}", role);
                self.skill_bank
                    .role_skills
                    .entry(role_key)
                    .or_default()
                    .push(skill.clone());
            }
            SkillLevel::TaskSpecific(ref task) => {
                self.skill_bank
                    .task_skills
                    .entry(task.clone())
                    .or_default()
                    .push(skill.clone());
            }
        }

        // Cache embedding
        self.embedding_cache
            .insert(skill.id.clone(), embedding.clone());

        // Add to semantic graph
        self.semantic_graph.add_skill_node(&skill, embedding);
        self.semantic_graph
            .update_relationships(&skill.id, &self.embedding_cache);

        Ok(())
    }

    /// Find skill composition paths (chains of skills that achieve a goal)
    pub fn find_composition_path(
        &self,
        start_skill: &str,
        goal_description: &str,
    ) -> Option<SkillChain> {
        self.semantic_graph
            .find_path(start_skill, goal_description, &self.embedding_cache)
    }

    /// Get skills related to a given skill
    pub fn related_skills(&self, skill_id: &str, limit: usize) -> Vec<RelatedSkill> {
        self.semantic_graph.get_related(skill_id, limit)
    }

    /// Update context for better skill retrieval
    pub fn update_context(&mut self, snapshot: ContextSnapshot) {
        self.context_manager.update(snapshot);
    }

    /// Number of known skills in the semantic registry.
    pub fn skill_count(&self) -> usize {
        self.skill_bank.all_skills().len()
    }

    /// Number of context snapshots retained for relevance scoring.
    pub fn context_depth(&self) -> usize {
        self.context_manager.history().len()
    }

    /// Embedding dimensionality currently used by CASS.
    pub fn embedding_dimension(&self) -> usize {
        self.embedding_cache
            .values()
            .next()
            .map(|e| e.dimension)
            .unwrap_or_else(|| self.embedding_engine.dimension())
    }

    /// Record skill usage for learning
    pub fn record_usage(&mut self, skill_id: &str, success: bool, context: ContextSnapshot) {
        self.context_manager
            .record_usage(skill_id, success, context);
        if success {
            self.semantic_graph.boost_skill(skill_id);
        }
    }

    /// Persist CASS state to RooDB
    pub async fn persist(&self, db: &crate::database::RooDb) -> anyhow::Result<()> {
        // Persist embedding cache
        for (skill_id, embedding) in &self.embedding_cache {
            let sql = format!(
                "INSERT INTO cass_embeddings (skill_id, embedding, updated_at) 
                 VALUES ('{}', '{}', {})
                 ON DUPLICATE KEY UPDATE 
                 embedding = VALUES(embedding), updated_at = VALUES(updated_at)",
                skill_id,
                serde_json::to_string(&embedding.vector)?,
                current_timestamp()
            );
            db.execute(&sql).await?;
        }

        // Persist semantic graph edges
        self.semantic_graph.persist(db).await?;

        Ok(())
    }

    /// Initialize CASS database schema
    pub async fn init_schema(db: &crate::database::RooDb) -> anyhow::Result<()> {
        let sql = r#"
            CREATE TABLE IF NOT EXISTS cass_embeddings (
                id BIGINT AUTO_INCREMENT PRIMARY KEY,
                skill_id VARCHAR(255) NOT NULL UNIQUE,
                embedding TEXT NOT NULL,
                updated_at BIGINT NOT NULL,
                INDEX idx_skill (skill_id)
            );
            
            CREATE TABLE IF NOT EXISTS cass_graph_edges (
                id BIGINT AUTO_INCREMENT PRIMARY KEY,
                source_skill VARCHAR(255) NOT NULL,
                target_skill VARCHAR(255) NOT NULL,
                edge_type VARCHAR(50) NOT NULL,
                weight DOUBLE NOT NULL,
                created_at BIGINT NOT NULL,
                UNIQUE KEY unique_edge (source_skill, target_skill, edge_type),
                INDEX idx_source (source_skill),
                INDEX idx_target (target_skill)
            )
        "#;
        db.execute(sql).await?;
        Ok(())
    }
}

/// A skill match with semantic scoring
#[derive(Clone, Debug)]
pub struct SemanticSkillMatch {
    pub skill: Skill,
    pub semantic_score: f64,
    pub context_relevance: f64,
    pub related_skills: Vec<String>,
}

/// A chain of composable skills
#[derive(Clone, Debug)]
pub struct SkillChain {
    pub skills: Vec<Skill>,
    pub total_confidence: f64,
    pub estimated_effectiveness: f64,
}

/// A related skill with relationship info
#[derive(Clone, Debug)]
pub struct RelatedSkill {
    pub skill_id: String,
    pub relationship: String,
    pub strength: f64,
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
