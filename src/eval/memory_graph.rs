//! Bipartite (entity–fact) encoding for HSM-II eval memory: beliefs, session boundaries, skills.
//!
//! **Entities** (`MemoryEntity`) live on one side; **reified statements** (`ReifiedFact`) on the other;
//! **`Incidence`** links them with roles (`subject`, `object`, `context`, …). This is the standard
//! RDF-style reification / statement-node trick — a workable SQL- and property-graph–friendly
//! projection of metagraph-like “edges as nodes” patterns.
//!
//! Layers (orthogonal to bipartite sets):
//! - **Episodic** — session slices and cross-session summary artifacts (boundaries).
//! - **Semantic** — durable beliefs and their sources.
//! - **Procedural** — skill catalog and usage/evidence links.

use serde::{Deserialize, Serialize};

/// Cognitive / storage layer tag (not the same as “side” of the bipartite graph).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLayer {
    Episodic,
    Semantic,
    Procedural,
}

impl MemoryLayer {
    pub fn as_sql(&self) -> &'static str {
        match self {
            MemoryLayer::Episodic => "episodic",
            MemoryLayer::Semantic => "semantic",
            MemoryLayer::Procedural => "procedural",
        }
    }
}

/// Serializable copy of a stored belief for export / graph projection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TypedClaimSnapshot {
    pub subject: String,
    pub relation: String,
    pub object: String,
    #[serde(default)]
    pub negated: bool,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub temporal_hint: Option<String>,
}

/// Serializable copy of a stored belief for export / graph projection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BeliefSnapshot {
    pub index: usize,
    pub content: String,
    pub confidence: f64,
    pub domain: Option<String>,
    pub source_task: String,
    pub source_turn: usize,
    pub created_at: u64,
    pub keywords: Vec<String>,
    #[serde(default)]
    pub source_excerpt: Option<String>,
    #[serde(default)]
    pub supporting_evidence: Vec<String>,
    #[serde(default)]
    pub contradicting_evidence: Vec<String>,
    #[serde(default)]
    pub supersedes_belief_index: Option<usize>,
    #[serde(default)]
    pub evidence_belief_indices: Vec<usize>,
    #[serde(default)]
    pub human_committed: bool,
    #[serde(default)]
    pub claims: Vec<TypedClaimSnapshot>,
}

/// One session-line summary row (cross-session recall).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSummarySnapshot {
    pub task_id: String,
    pub session: u32,
    pub summary: String,
    pub key_decisions: Vec<String>,
    pub keywords: Vec<String>,
}

/// Tracked skill row for procedural layer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillSnapshot {
    pub id: String,
    pub description: String,
    pub domain: String,
    pub usage_count: u64,
    pub success_count: u64,
    pub avg_keyword_score: f64,
}

/// Point-in-time view of HSM runner memory suitable for bipartite projection.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HsmMemorySnapshot {
    pub beliefs: Vec<BeliefSnapshot>,
    pub session_summaries: Vec<SessionSummarySnapshot>,
    pub skills: Vec<SkillSnapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryEntity {
    pub id: String,
    pub layer: MemoryLayer,
    /// `task`, `session_slice`, `belief`, `skill`, `keyword`, `domain`
    pub kind: String,
    pub label: Option<String>,
    #[serde(default)]
    pub properties: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReifiedFact {
    pub id: String,
    pub layer: MemoryLayer,
    /// Named relation type for the reified hyperedge (e.g. `belief_asserted`, `session_summarized`).
    pub relation: String,
    #[serde(default)]
    pub properties: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Incidence {
    pub entity_id: String,
    pub fact_id: String,
    /// `subject` | `object` | `context` | `source_task` | `source_session` | `skill` | `keyword` | …
    pub role: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BipartiteMemoryGraph {
    pub entities: Vec<MemoryEntity>,
    pub facts: Vec<ReifiedFact>,
    pub incidence: Vec<Incidence>,
}

fn entity_task(task_id: &str) -> String {
    format!("ent:task:{task_id}")
}

fn entity_session(task_id: &str, session: u32) -> String {
    format!("ent:session:{task_id}:{session}")
}

fn entity_belief(idx: usize) -> String {
    format!("ent:belief:{idx}")
}

fn entity_skill(skill_id: &str) -> String {
    format!("ent:skill:{skill_id}")
}

fn entity_domain(domain: &str) -> String {
    format!("ent:domain:{domain}")
}

fn entity_keyword(task_id: &str, kw: &str) -> String {
    let safe = kw.replace(['/', '\\', ' '], "_");
    format!("ent:keyword:{task_id}:{safe}")
}

fn entity_eval_turn(task_id: &str, turn_index: usize) -> String {
    format!("ent:eval_turn:{task_id}:{turn_index}")
}

fn entity_summary(task_id: &str, session: u32, row: usize) -> String {
    format!("ent:summary:{task_id}:{session}:{row}")
}

fn entity_decision(task_id: &str, session: u32, row: usize, decision_index: usize) -> String {
    format!("ent:decision:{task_id}:{session}:{row}:{decision_index}")
}

fn entity_contradiction_note(belief_index: usize, note_index: usize) -> String {
    format!("ent:contradiction:{belief_index}:{note_index}")
}

fn entity_supporting_evidence(belief_index: usize, evidence_index: usize) -> String {
    format!("ent:supporting_evidence:{belief_index}:{evidence_index}")
}

impl BipartiteMemoryGraph {
    /// Build bipartite entity–fact graph from an HSM memory snapshot.
    pub fn project_from_snapshot(snap: &HsmMemorySnapshot) -> Self {
        let mut g = BipartiteMemoryGraph::default();
        let mut seen_entity: std::collections::HashSet<String> = std::collections::HashSet::new();

        let mut add_ent = |g: &mut BipartiteMemoryGraph, e: MemoryEntity| {
            if seen_entity.insert(e.id.clone()) {
                g.entities.push(e);
            }
        };

        // ── Skills (procedural catalog + domain anchoring) ──
        for sk in &snap.skills {
            let sid = entity_skill(&sk.id);
            add_ent(
                &mut g,
                MemoryEntity {
                    id: sid.clone(),
                    layer: MemoryLayer::Procedural,
                    kind: "skill".into(),
                    label: Some(sk.id.clone()),
                    properties: serde_json::json!({
                        "description": sk.description,
                        "domain": sk.domain,
                        "usage_count": sk.usage_count,
                        "success_count": sk.success_count,
                        "avg_keyword_score": sk.avg_keyword_score,
                    }),
                },
            );
            let dom_id = entity_domain(&sk.domain);
            add_ent(
                &mut g,
                MemoryEntity {
                    id: dom_id.clone(),
                    layer: MemoryLayer::Procedural,
                    kind: "domain".into(),
                    label: Some(sk.domain.clone()),
                    properties: serde_json::json!({}),
                },
            );
            let fid = format!("fact:skill_expertise:{}:{}", sk.id, sk.domain);
            g.facts.push(ReifiedFact {
                id: fid.clone(),
                layer: MemoryLayer::Procedural,
                relation: "expertise_for_domain".into(),
                properties: serde_json::json!({}),
            });
            g.incidence.push(Incidence {
                entity_id: sid,
                fact_id: fid.clone(),
                role: "skill".into(),
            });
            g.incidence.push(Incidence {
                entity_id: dom_id,
                fact_id: fid,
                role: "domain".into(),
            });
        }

        // ── Beliefs (semantic assertions, sourced to task / optional domain) ──
        for b in &snap.beliefs {
            let bid = entity_belief(b.index);
            add_ent(
                &mut g,
                MemoryEntity {
                    id: bid.clone(),
                    layer: MemoryLayer::Semantic,
                    kind: "belief".into(),
                    label: Some(truncate_label(&b.content, 80)),
                    properties: serde_json::json!({
                        "content": b.content,
                        "confidence": b.confidence,
                        "source_turn": b.source_turn,
                        "created_at": b.created_at,
                        "keywords": b.keywords,
                        "human_committed": b.human_committed,
                        "claims": b.claims,
                    }),
                },
            );
            let tid = entity_task(&b.source_task);
            add_ent(
                &mut g,
                MemoryEntity {
                    id: tid.clone(),
                    layer: MemoryLayer::Episodic,
                    kind: "task".into(),
                    label: Some(b.source_task.clone()),
                    properties: serde_json::json!({}),
                },
            );
            let evidence_turn_id = entity_eval_turn(&b.source_task, b.source_turn);
            add_ent(
                &mut g,
                MemoryEntity {
                    id: evidence_turn_id.clone(),
                    layer: MemoryLayer::Episodic,
                    kind: "evidence_turn".into(),
                    label: Some(format!("{}:{}", b.source_task, b.source_turn)),
                    properties: serde_json::json!({
                        "task_id": b.source_task,
                        "turn_index": b.source_turn,
                        "excerpt": b.source_excerpt,
                        "immutable_source": true,
                    }),
                },
            );

            let fid = format!("fact:belief_asserted:{}", b.index);
            g.facts.push(ReifiedFact {
                id: fid.clone(),
                layer: MemoryLayer::Semantic,
                relation: "belief_asserted".into(),
                properties: serde_json::json!({
                    "confidence": b.confidence,
                    "source_turn": b.source_turn,
                    "created_at": b.created_at,
                }),
            });
            g.incidence.push(Incidence {
                entity_id: bid,
                fact_id: fid.clone(),
                role: "subject".into(),
            });
            g.incidence.push(Incidence {
                entity_id: tid,
                fact_id: fid.clone(),
                role: "source_task".into(),
            });
            g.incidence.push(Incidence {
                entity_id: evidence_turn_id.clone(),
                fact_id: fid.clone(),
                role: "source_evidence".into(),
            });
            if let Some(ref dom) = b.domain {
                let did = entity_domain(dom);
                add_ent(
                    &mut g,
                    MemoryEntity {
                        id: did.clone(),
                        layer: MemoryLayer::Semantic,
                        kind: "domain".into(),
                        label: Some(dom.clone()),
                        properties: serde_json::json!({}),
                    },
                );
                g.incidence.push(Incidence {
                    entity_id: did,
                    fact_id: fid,
                    role: "context".into(),
                });
            }
            for kw in &b.keywords {
                let kid = entity_keyword(&b.source_task, kw);
                add_ent(
                    &mut g,
                    MemoryEntity {
                        id: kid.clone(),
                        layer: MemoryLayer::Semantic,
                        kind: "keyword".into(),
                        label: Some(kw.clone()),
                        properties: serde_json::json!({}),
                    },
                );
                let fkw = format!("fact:belief_keyword:{}:{}", b.index, kid);
                g.facts.push(ReifiedFact {
                    id: fkw.clone(),
                    layer: MemoryLayer::Semantic,
                    relation: "belief_supports_keyword".into(),
                    properties: serde_json::json!({}),
                });
                g.incidence.push(Incidence {
                    entity_id: entity_belief(b.index),
                    fact_id: fkw.clone(),
                    role: "belief".into(),
                });
                g.incidence.push(Incidence {
                    entity_id: kid,
                    fact_id: fkw,
                    role: "keyword".into(),
                });
            }
            if let Some(superseded_index) = b.supersedes_belief_index {
                let older_belief_id = entity_belief(superseded_index);
                add_ent(
                    &mut g,
                    MemoryEntity {
                        id: older_belief_id.clone(),
                        layer: MemoryLayer::Semantic,
                        kind: "belief".into(),
                        label: Some(format!("belief {superseded_index}")),
                        properties: serde_json::json!({
                            "placeholder": true,
                            "belief_index": superseded_index,
                        }),
                    },
                );
                let fs = format!("fact:belief_supersedes:{}:{}", b.index, superseded_index);
                g.facts.push(ReifiedFact {
                    id: fs.clone(),
                    layer: MemoryLayer::Semantic,
                    relation: "belief_supersedes_belief".into(),
                    properties: serde_json::json!({}),
                });
                g.incidence.push(Incidence {
                    entity_id: entity_belief(b.index),
                    fact_id: fs.clone(),
                    role: "newer_belief".into(),
                });
                g.incidence.push(Incidence {
                    entity_id: older_belief_id,
                    fact_id: fs,
                    role: "older_belief".into(),
                });
            }
            for evidence_belief_index in &b.evidence_belief_indices {
                let evidence_belief_id = entity_belief(*evidence_belief_index);
                add_ent(
                    &mut g,
                    MemoryEntity {
                        id: evidence_belief_id.clone(),
                        layer: MemoryLayer::Semantic,
                        kind: "belief".into(),
                        label: Some(format!("belief {evidence_belief_index}")),
                        properties: serde_json::json!({
                            "placeholder": true,
                            "belief_index": evidence_belief_index,
                        }),
                    },
                );
                let fe = format!(
                    "fact:belief_supported_by_belief:{}:{}",
                    b.index, evidence_belief_index
                );
                g.facts.push(ReifiedFact {
                    id: fe.clone(),
                    layer: MemoryLayer::Semantic,
                    relation: "belief_supported_by_belief".into(),
                    properties: serde_json::json!({}),
                });
                g.incidence.push(Incidence {
                    entity_id: entity_belief(b.index),
                    fact_id: fe.clone(),
                    role: "supported_belief".into(),
                });
                g.incidence.push(Incidence {
                    entity_id: evidence_belief_id,
                    fact_id: fe,
                    role: "supporting_belief".into(),
                });
            }
            for (support_index, evidence) in b.supporting_evidence.iter().enumerate() {
                let evidence_id = entity_supporting_evidence(b.index, support_index);
                add_ent(
                    &mut g,
                    MemoryEntity {
                        id: evidence_id.clone(),
                        layer: MemoryLayer::Semantic,
                        kind: "supporting_evidence".into(),
                        label: Some(truncate_label(evidence, 80)),
                        properties: serde_json::json!({
                            "content": evidence,
                        }),
                    },
                );
                let fe = format!(
                    "fact:belief_supporting_evidence:{}:{}",
                    b.index, support_index
                );
                g.facts.push(ReifiedFact {
                    id: fe.clone(),
                    layer: MemoryLayer::Semantic,
                    relation: "belief_supported_by_evidence".into(),
                    properties: serde_json::json!({}),
                });
                g.incidence.push(Incidence {
                    entity_id: entity_belief(b.index),
                    fact_id: fe.clone(),
                    role: "supported_belief".into(),
                });
                g.incidence.push(Incidence {
                    entity_id: evidence_id,
                    fact_id: fe,
                    role: "supporting_evidence".into(),
                });
            }
            for (note_index, note) in b.contradicting_evidence.iter().enumerate() {
                let note_id = entity_contradiction_note(b.index, note_index);
                add_ent(
                    &mut g,
                    MemoryEntity {
                        id: note_id.clone(),
                        layer: MemoryLayer::Semantic,
                        kind: "contradiction_note".into(),
                        label: Some(truncate_label(note, 80)),
                        properties: serde_json::json!({
                            "content": note,
                        }),
                    },
                );
                let fc = format!("fact:belief_contradicted:{}:{}", b.index, note_index);
                g.facts.push(ReifiedFact {
                    id: fc.clone(),
                    layer: MemoryLayer::Semantic,
                    relation: "belief_flagged_by_contradiction".into(),
                    properties: serde_json::json!({}),
                });
                g.incidence.push(Incidence {
                    entity_id: entity_belief(b.index),
                    fact_id: fc.clone(),
                    role: "contested_belief".into(),
                });
                g.incidence.push(Incidence {
                    entity_id: note_id,
                    fact_id: fc,
                    role: "contradicting_evidence".into(),
                });
            }
        }

        // ── Session summaries (episodic boundary artifacts) ──
        for (row, ss) in snap.session_summaries.iter().enumerate() {
            let sid = entity_session(&ss.task_id, ss.session);
            add_ent(
                &mut g,
                MemoryEntity {
                    id: sid.clone(),
                    layer: MemoryLayer::Episodic,
                    kind: "session_slice".into(),
                    label: Some(format!("{}:{}", ss.task_id, ss.session)),
                    properties: serde_json::json!({
                        "summary": ss.summary,
                        "key_decisions": ss.key_decisions,
                    }),
                },
            );
            let tid = entity_task(&ss.task_id);
            add_ent(
                &mut g,
                MemoryEntity {
                    id: tid.clone(),
                    layer: MemoryLayer::Episodic,
                    kind: "task".into(),
                    label: Some(ss.task_id.clone()),
                    properties: serde_json::json!({}),
                },
            );
            let summary_id = entity_summary(&ss.task_id, ss.session, row);
            add_ent(
                &mut g,
                MemoryEntity {
                    id: summary_id.clone(),
                    layer: MemoryLayer::Episodic,
                    kind: "summary".into(),
                    label: Some(truncate_label(&ss.summary, 80)),
                    properties: serde_json::json!({
                        "summary": ss.summary,
                        "session": ss.session,
                        "decision_count": ss.key_decisions.len(),
                        "keyword_count": ss.keywords.len(),
                        "index_only": true,
                    }),
                },
            );
            let fid = format!(
                "fact:session_boundary:{}:{}:{}",
                ss.task_id, ss.session, row
            );
            g.facts.push(ReifiedFact {
                id: fid.clone(),
                layer: MemoryLayer::Episodic,
                relation: "session_summarized_at_boundary".into(),
                properties: serde_json::json!({ "session": ss.session }),
            });
            g.incidence.push(Incidence {
                entity_id: sid,
                fact_id: fid.clone(),
                role: "subject".into(),
            });
            g.incidence.push(Incidence {
                entity_id: tid,
                fact_id: fid,
                role: "contained_under_task".into(),
            });
            let fs = format!(
                "fact:summary_indexes_session:{}:{}:{}",
                ss.task_id, ss.session, row
            );
            g.facts.push(ReifiedFact {
                id: fs.clone(),
                layer: MemoryLayer::Episodic,
                relation: "summary_indexes_session_slice".into(),
                properties: serde_json::json!({ "session": ss.session }),
            });
            g.incidence.push(Incidence {
                entity_id: summary_id.clone(),
                fact_id: fs.clone(),
                role: "summary".into(),
            });
            g.incidence.push(Incidence {
                entity_id: entity_session(&ss.task_id, ss.session),
                fact_id: fs,
                role: "session_slice".into(),
            });
            for kw in &ss.keywords {
                let kid = entity_keyword(&ss.task_id, kw);
                add_ent(
                    &mut g,
                    MemoryEntity {
                        id: kid.clone(),
                        layer: MemoryLayer::Episodic,
                        kind: "keyword".into(),
                        label: Some(kw.clone()),
                        properties: serde_json::json!({}),
                    },
                );
                let fk = format!(
                    "fact:session_keyword:{}:{}:{}:{}",
                    ss.task_id, ss.session, row, kid
                );
                g.facts.push(ReifiedFact {
                    id: fk.clone(),
                    layer: MemoryLayer::Episodic,
                    relation: "session_slice_supports_keyword".into(),
                    properties: serde_json::json!({}),
                });
                g.incidence.push(Incidence {
                    entity_id: entity_session(&ss.task_id, ss.session),
                    fact_id: fk.clone(),
                    role: "session_slice".into(),
                });
                g.incidence.push(Incidence {
                    entity_id: kid,
                    fact_id: fk,
                    role: "keyword".into(),
                });
            }
            for (decision_index, decision) in ss.key_decisions.iter().enumerate() {
                let decision_id = entity_decision(&ss.task_id, ss.session, row, decision_index);
                add_ent(
                    &mut g,
                    MemoryEntity {
                        id: decision_id.clone(),
                        layer: MemoryLayer::Episodic,
                        kind: "decision".into(),
                        label: Some(truncate_label(decision, 80)),
                        properties: serde_json::json!({
                            "content": decision,
                            "task_id": ss.task_id,
                            "session": ss.session,
                        }),
                    },
                );
                let fd = format!(
                    "fact:summary_indexes_decision:{}:{}:{}:{}",
                    ss.task_id, ss.session, row, decision_index
                );
                g.facts.push(ReifiedFact {
                    id: fd.clone(),
                    layer: MemoryLayer::Episodic,
                    relation: "summary_indexes_decision".into(),
                    properties: serde_json::json!({}),
                });
                g.incidence.push(Incidence {
                    entity_id: summary_id.clone(),
                    fact_id: fd.clone(),
                    role: "summary".into(),
                });
                g.incidence.push(Incidence {
                    entity_id: decision_id.clone(),
                    fact_id: fd.clone(),
                    role: "decision".into(),
                });
                let fr = format!(
                    "fact:decision_recorded_in_session:{}:{}:{}:{}",
                    ss.task_id, ss.session, row, decision_index
                );
                g.facts.push(ReifiedFact {
                    id: fr.clone(),
                    layer: MemoryLayer::Episodic,
                    relation: "decision_recorded_in_session".into(),
                    properties: serde_json::json!({ "session": ss.session }),
                });
                g.incidence.push(Incidence {
                    entity_id: decision_id,
                    fact_id: fr.clone(),
                    role: "decision".into(),
                });
                g.incidence.push(Incidence {
                    entity_id: entity_session(&ss.task_id, ss.session),
                    fact_id: fr,
                    role: "session_slice".into(),
                });
            }
        }

        g
    }

    /// Same as [`Self::project_from_snapshot`] then [`Self::append_traces`].
    pub fn project_from_snapshot_with_traces(
        snap: &HsmMemorySnapshot,
        traces: &[super::trace::HsmTurnTrace],
    ) -> Self {
        let mut g = Self::project_from_snapshot(snap);
        g.append_traces(traces);
        g
    }

    /// Append eval-turn / retrieval facts from harness traces (`--trace` JSONL rows).
    pub fn append_traces(&mut self, traces: &[super::trace::HsmTurnTrace]) {
        let mut seen_entity: std::collections::HashSet<String> =
            self.entities.iter().map(|e| e.id.clone()).collect();

        fn push_ent(
            g: &mut BipartiteMemoryGraph,
            seen: &mut std::collections::HashSet<String>,
            e: MemoryEntity,
        ) {
            if seen.insert(e.id.clone()) {
                g.entities.push(e);
            }
        }

        for tr in traces {
            let eid = entity_eval_turn(&tr.task_id, tr.turn_index);
            push_ent(
                self,
                &mut seen_entity,
                MemoryEntity {
                    id: eid.clone(),
                    layer: MemoryLayer::Episodic,
                    kind: "eval_turn".into(),
                    label: Some(format!("{}:{}", tr.task_id, tr.turn_index)),
                    properties: serde_json::json!({
                        "session": tr.session,
                        "requires_recall": tr.requires_recall,
                        "injected_char_len": tr.injected_char_len,
                        "session_compaction_applied": tr.session_compaction_applied,
                        "session_history_len": tr.session_history_len,
                        "injected_preview": truncate_label(&tr.injected_preview, 2000),
                    }),
                },
            );

            let tid = entity_task(&tr.task_id);
            push_ent(
                self,
                &mut seen_entity,
                MemoryEntity {
                    id: tid.clone(),
                    layer: MemoryLayer::Episodic,
                    kind: "task".into(),
                    label: Some(tr.task_id.clone()),
                    properties: serde_json::json!({}),
                },
            );

            let fid = format!("fact:retrieval_turn:{}:{}", tr.task_id, tr.turn_index);
            self.facts.push(ReifiedFact {
                id: fid.clone(),
                layer: MemoryLayer::Semantic,
                relation: "retrieval_turn".into(),
                properties: serde_json::json!({
                    "session": tr.session,
                    "requires_recall": tr.requires_recall,
                    "injected_char_len": tr.injected_char_len,
                }),
            });

            self.incidence.push(Incidence {
                entity_id: eid.clone(),
                fact_id: fid.clone(),
                role: "subject".into(),
            });
            self.incidence.push(Incidence {
                entity_id: tid,
                fact_id: fid.clone(),
                role: "task".into(),
            });

            if let Some(ref skill_id) = tr.selected_skill_id {
                let sid = entity_skill(skill_id);
                push_ent(
                    self,
                    &mut seen_entity,
                    MemoryEntity {
                        id: sid.clone(),
                        layer: MemoryLayer::Procedural,
                        kind: "skill".into(),
                        label: Some(skill_id.clone()),
                        properties: serde_json::json!({
                            "selected_on_turn": true,
                            "domain": tr.selected_skill_domain,
                        }),
                    },
                );
                self.incidence.push(Incidence {
                    entity_id: sid,
                    fact_id: fid.clone(),
                    role: "selected_skill".into(),
                });
            }

            for (rank, br) in tr.belief_ranks.iter().enumerate() {
                let bid = entity_belief(br.belief_index);
                push_ent(
                    self,
                    &mut seen_entity,
                    MemoryEntity {
                        id: bid.clone(),
                        layer: MemoryLayer::Semantic,
                        kind: "belief".into(),
                        label: Some(truncate_label(&br.preview, 80)),
                        properties: serde_json::json!({
                            "preview": br.preview,
                            "source_task_rank": br.source_task,
                        }),
                    },
                );
                let rid = format!(
                    "fact:retrieval_rank:{}:{}:{}",
                    tr.task_id, tr.turn_index, br.belief_index
                );
                self.facts.push(ReifiedFact {
                    id: rid.clone(),
                    layer: MemoryLayer::Semantic,
                    relation: "ranked_belief_at_turn".into(),
                    properties: serde_json::json!({
                        "score": br.score,
                        "rank": rank,
                    }),
                });
                self.incidence.push(Incidence {
                    entity_id: bid,
                    fact_id: rid.clone(),
                    role: "ranked_belief".into(),
                });
                self.incidence.push(Incidence {
                    entity_id: eid.clone(),
                    fact_id: rid,
                    role: "at_turn".into(),
                });
            }

            for sess in &tr.session_summaries_injected {
                let ses_ent = entity_session(&tr.task_id, *sess);
                push_ent(
                    self,
                    &mut seen_entity,
                    MemoryEntity {
                        id: ses_ent.clone(),
                        layer: MemoryLayer::Episodic,
                        kind: "session_slice".into(),
                        label: Some(format!("{}:{}", tr.task_id, sess)),
                        properties: serde_json::json!({ "injected_from_trace": true }),
                    },
                );
                let inj_id = format!(
                    "fact:retrieval_injected_session:{}:{}:{}",
                    tr.task_id, tr.turn_index, sess
                );
                self.facts.push(ReifiedFact {
                    id: inj_id.clone(),
                    layer: MemoryLayer::Episodic,
                    relation: "injected_session_context_at_turn".into(),
                    properties: serde_json::json!({ "session": sess }),
                });
                self.incidence.push(Incidence {
                    entity_id: ses_ent,
                    fact_id: inj_id.clone(),
                    role: "session_slice".into(),
                });
                self.incidence.push(Incidence {
                    entity_id: eid.clone(),
                    fact_id: inj_id,
                    role: "at_turn".into(),
                });
            }
        }
    }
}

fn truncate_label(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_covers_layers_and_bipartite_roles() {
        let snap = HsmMemorySnapshot {
            beliefs: vec![BeliefSnapshot {
                index: 0,
                content: "API uses JWT".into(),
                confidence: 0.9,
                domain: Some("software_engineering".into()),
                source_task: "se-01".into(),
                source_turn: 2,
                created_at: 1,
                keywords: vec!["jwt".into()],
                source_excerpt: Some("User specified JWT auth in turn 2".into()),
                supporting_evidence: vec!["RFC 7519 referenced".into()],
                contradicting_evidence: vec!["Legacy doc still says cookie sessions".into()],
                supersedes_belief_index: Some(9),
                evidence_belief_indices: vec![7],
                human_committed: true,
                claims: vec![TypedClaimSnapshot {
                    subject: "task:se-01".into(),
                    relation: "auth_method".into(),
                    object: "jwt".into(),
                    negated: false,
                    scope: Some("software_engineering".into()),
                    temporal_hint: None,
                }],
            }],
            session_summaries: vec![SessionSummarySnapshot {
                task_id: "se-01".into(),
                session: 1,
                summary: "Designed API".into(),
                key_decisions: vec!["Use JWT for auth".into()],
                keywords: vec![],
            }],
            skills: vec![SkillSnapshot {
                id: "api-design".into(),
                description: "REST".into(),
                domain: "software_engineering".into(),
                usage_count: 1,
                success_count: 1,
                avg_keyword_score: 0.8,
            }],
        };
        let g = BipartiteMemoryGraph::project_from_snapshot(&snap);
        assert!(!g.entities.is_empty());
        assert!(!g.facts.is_empty());
        assert!(!g.incidence.is_empty());
        let layers: std::collections::HashSet<_> = g.entities.iter().map(|e| e.layer).collect();
        assert!(layers.contains(&MemoryLayer::Episodic));
        assert!(layers.contains(&MemoryLayer::Semantic));
        assert!(layers.contains(&MemoryLayer::Procedural));
        assert!(g.incidence.iter().any(|i| i.role == "subject"));
        assert!(g.facts.iter().any(|f| f.relation == "belief_asserted"));
        assert!(g
            .facts
            .iter()
            .any(|f| f.relation == "session_summarized_at_boundary"));
        assert!(g.facts.iter().any(|f| f.relation == "expertise_for_domain"));
        assert!(g
            .entities
            .iter()
            .any(|e| e.kind == "evidence_turn" && e.id == "ent:eval_turn:se-01:2"));
        assert!(g
            .facts
            .iter()
            .any(|f| f.relation == "belief_supersedes_belief"));
        assert!(g
            .facts
            .iter()
            .any(|f| f.relation == "summary_indexes_decision"));
        assert!(g
            .incidence
            .iter()
            .any(|i| i.role == "source_evidence" && i.entity_id == "ent:eval_turn:se-01:2"));
        let belief = g
            .entities
            .iter()
            .find(|e| e.id == "ent:belief:0")
            .expect("belief entity should exist");
        assert_eq!(
            belief.properties["claims"][0]["relation"].as_str(),
            Some("auth_method")
        );
    }

    #[test]
    fn append_traces_adds_retrieval_and_rank_facts() {
        use super::super::trace::{BeliefRankEntry, HsmTurnTrace};
        let snap = HsmMemorySnapshot::default();
        let traces = vec![HsmTurnTrace {
            task_id: "t".into(),
            turn_index: 0,
            session: 1,
            requires_recall: true,
            selected_skill_id: None,
            selected_skill_domain: None,
            belief_ranks: vec![BeliefRankEntry {
                belief_index: 7,
                score: 0.5,
                source_task: "t".into(),
                preview: "p".into(),
            }],
            session_summaries_injected: vec![],
            injected_char_len: 0,
            injected_preview: String::new(),
            session_compaction_applied: false,
            session_history_len: 0,
        }];
        let g = BipartiteMemoryGraph::project_from_snapshot_with_traces(&snap, &traces);
        assert!(g.facts.iter().any(|f| f.relation == "retrieval_turn"));
        assert!(g
            .facts
            .iter()
            .any(|f| f.relation == "ranked_belief_at_turn"));
        assert!(g
            .incidence
            .iter()
            .any(|i| i.role == "ranked_belief" && i.entity_id == "ent:belief:7"));
    }
}
