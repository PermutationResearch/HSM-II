use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::agent::AgentId;
use crate::hyper_stigmergy::HyperStigmergicMorphogenesis;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PropertyValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    StringList(Vec<String>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphNodeRecord {
    pub id: String,
    pub labels: Vec<String>,
    pub properties: HashMap<String, PropertyValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphRelationshipRecord {
    pub id: String,
    pub start_node: String,
    pub end_node: String,
    pub rel_type: String,
    pub properties: HashMap<String, PropertyValue>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PropertyGraphSnapshot {
    pub nodes: Vec<GraphNodeRecord>,
    pub relationships: Vec<GraphRelationshipRecord>,
}

impl PropertyGraphSnapshot {
    pub fn find_node(&self, id: &str) -> Option<&GraphNodeRecord> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn nodes_with_label<'a>(&'a self, label: &str) -> Vec<&'a GraphNodeRecord> {
        self.nodes
            .iter()
            .filter(|n| n.labels.iter().any(|l| l == label))
            .collect()
    }
}

impl HyperStigmergicMorphogenesis {
    pub fn to_property_graph_snapshot(&self) -> PropertyGraphSnapshot {
        let mut nodes = Vec::new();
        let mut relationships = Vec::new();

        for agent in &self.agents {
            let mut properties = HashMap::new();
            properties.insert("agent_id".into(), PropertyValue::Integer(agent.id as i64));
            properties.insert(
                "role".into(),
                PropertyValue::String(format!("{:?}", agent.role)),
            );
            properties.insert(
                "description".into(),
                PropertyValue::String(agent.description.clone()),
            );
            properties.insert("jw".into(), PropertyValue::Float(agent.jw));
            properties.insert(
                "curiosity".into(),
                PropertyValue::Float(agent.drives.curiosity),
            );
            properties.insert("harmony".into(), PropertyValue::Float(agent.drives.harmony));
            properties.insert("growth".into(), PropertyValue::Float(agent.drives.growth));
            properties.insert(
                "transcendence".into(),
                PropertyValue::Float(agent.drives.transcendence),
            );
            nodes.push(GraphNodeRecord {
                id: agent_node_id(agent.id),
                labels: vec!["Agent".into(), format!("{:?}", agent.role)],
                properties,
            });
        }

        for (idx, meta) in self.vertex_meta.iter().enumerate() {
            let mut properties = HashMap::new();
            properties.insert("vertex_index".into(), PropertyValue::Integer(idx as i64));
            properties.insert("name".into(), PropertyValue::String(meta.name.clone()));
            properties.insert(
                "created_at".into(),
                PropertyValue::Integer(meta.created_at as i64),
            );
            properties.insert(
                "modified_at".into(),
                PropertyValue::Integer(meta.modified_at as i64),
            );
            properties.insert(
                "drift_count".into(),
                PropertyValue::Integer(meta.drift_count as i64),
            );
            nodes.push(GraphNodeRecord {
                id: vertex_node_id(idx),
                labels: vec!["Vertex".into(), format!("{:?}", meta.kind)],
                properties,
            });
        }

        for (concept, entry) in &self.ontology {
            let mut properties = HashMap::new();
            properties.insert("concept".into(), PropertyValue::String(concept.clone()));
            properties.insert(
                "confidence".into(),
                PropertyValue::Float(entry.confidence as f64),
            );
            properties.insert(
                "instances".into(),
                PropertyValue::StringList(entry.instances.clone()),
            );
            nodes.push(GraphNodeRecord {
                id: ontology_node_id(concept),
                labels: vec!["Ontology".into(), "Concept".into()],
                properties,
            });
        }

        for (prop_name, vertex_idx) in &self.property_vertices {
            relationships.push(GraphRelationshipRecord {
                id: format!("property-anchor-{}", prop_name),
                start_node: vertex_node_id(*vertex_idx),
                end_node: ontology_node_id("Property"),
                rel_type: "INSTANCE_OF".into(),
                properties: HashMap::new(),
            });
        }

        for edge_idx in 0..self.edges.len() {
            let edge = &self.edges[edge_idx];
            let participant_pairs = edge
                .participants
                .iter()
                .enumerate()
                .flat_map(|(i, &a)| edge.participants.iter().skip(i + 1).map(move |&b| (a, b)))
                .collect::<Vec<_>>();

            for (pair_idx, (a, b)) in participant_pairs.into_iter().enumerate() {
                let mut properties = HashMap::new();
                properties.insert("weight".into(), PropertyValue::Float(edge.weight));
                properties.insert("emergent".into(), PropertyValue::Boolean(edge.emergent));
                properties.insert("age".into(), PropertyValue::Integer(edge.age as i64));
                properties.insert(
                    "tags".into(),
                    PropertyValue::StringList(
                        edge.tags
                            .iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect::<Vec<_>>(),
                    ),
                );
                relationships.push(GraphRelationshipRecord {
                    id: format!("edge-{}-{}", edge_idx, pair_idx),
                    start_node: agent_node_id(a),
                    end_node: agent_node_id(b),
                    rel_type: "HYPEREDGE_LINK".into(),
                    properties,
                });
            }
        }

        for (idx, belief) in self.beliefs.iter().enumerate() {
            let mut properties = HashMap::new();
            properties.insert(
                "content".into(),
                PropertyValue::String(belief.content.clone()),
            );
            properties.insert("confidence".into(), PropertyValue::Float(belief.confidence));
            if let Some(ref o) = belief.owner_namespace {
                properties.insert("owner_namespace".into(), PropertyValue::String(o.clone()));
            }
            if let Some(sid) = belief.supersedes_belief_id {
                properties.insert(
                    "supersedes_belief_id".into(),
                    PropertyValue::Integer(sid as i64),
                );
            }
            if !belief.evidence_belief_ids.is_empty() {
                properties.insert(
                    "evidence_belief_ids".into(),
                    PropertyValue::String(
                        belief
                            .evidence_belief_ids
                            .iter()
                            .map(|i| i.to_string())
                            .collect::<Vec<_>>()
                            .join(","),
                    ),
                );
            }
            properties.insert(
                "human_committed".into(),
                PropertyValue::Boolean(belief.human_committed),
            );
            nodes.push(GraphNodeRecord {
                id: format!("belief:{idx}"),
                labels: vec!["Belief".into()],
                properties,
            });
        }

        for (idx, exp) in self.experiences.iter().enumerate() {
            let mut properties = HashMap::new();
            properties.insert(
                "description".into(),
                PropertyValue::String(exp.description.clone()),
            );
            properties.insert("context".into(), PropertyValue::String(exp.context.clone()));
            properties.insert("tick".into(), PropertyValue::Integer(exp.tick as i64));
            nodes.push(GraphNodeRecord {
                id: format!("experience:{idx}"),
                labels: vec!["Experience".into()],
                properties,
            });
        }

        for trace in &self.stigmergic_memory.traces {
            let mut properties = HashMap::new();
            properties.insert("trace_id".into(), PropertyValue::String(trace.id.clone()));
            properties.insert(
                "agent_id".into(),
                PropertyValue::Integer(trace.agent_id as i64),
            );
            properties.insert(
                "model_id".into(),
                PropertyValue::String(trace.model_id.clone()),
            );
            if let Some(task_key) = &trace.task_key {
                properties.insert("task_key".into(), PropertyValue::String(task_key.clone()));
            }
            properties.insert(
                "kind".into(),
                PropertyValue::String(trace.kind.as_str().to_string()),
            );
            properties.insert(
                "summary".into(),
                PropertyValue::String(trace.summary.clone()),
            );
            if let Some(success) = trace.success {
                properties.insert("success".into(), PropertyValue::Boolean(success));
            }
            if let Some(outcome_score) = trace.outcome_score {
                properties.insert("outcome_score".into(), PropertyValue::Float(outcome_score));
            }
            properties.insert(
                "sensitivity".into(),
                PropertyValue::String(format!("{:?}", trace.sensitivity)),
            );
            if let Some(planned_tool) = &trace.planned_tool {
                properties.insert(
                    "planned_tool".into(),
                    PropertyValue::String(planned_tool.clone()),
                );
            }
            properties.insert(
                "recorded_at".into(),
                PropertyValue::Integer(trace.recorded_at as i64),
            );
            properties.insert("tick".into(), PropertyValue::Integer(trace.tick as i64));
            properties.insert(
                "metadata".into(),
                PropertyValue::StringList(
                    trace
                        .metadata
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect(),
                ),
            );
            nodes.push(GraphNodeRecord {
                id: format!("trace:{}", trace.id),
                labels: vec!["Trace".into(), "StigmergicTrace".into()],
                properties,
            });
            relationships.push(GraphRelationshipRecord {
                id: format!("agent-trace-{}", trace.id),
                start_node: agent_node_id(trace.agent_id),
                end_node: format!("trace:{}", trace.id),
                rel_type: "EMITTED_TRACE".into(),
                properties: HashMap::new(),
            });
        }

        for directive in self.stigmergic_memory.directives.values() {
            let mut properties = HashMap::new();
            properties.insert(
                "task_key".into(),
                PropertyValue::String(directive.task_key.clone()),
            );
            if let Some(preferred_agent) = directive.preferred_agent {
                properties.insert(
                    "preferred_agent".into(),
                    PropertyValue::Integer(preferred_agent as i64),
                );
            }
            properties.insert(
                "preferred_tool".into(),
                PropertyValue::String(directive.preferred_tool.clone()),
            );
            properties.insert(
                "minimum_sensitivity".into(),
                PropertyValue::String(format!("{:?}", directive.minimum_sensitivity)),
            );
            properties.insert(
                "confidence".into(),
                PropertyValue::Float(directive.confidence),
            );
            properties.insert(
                "rationale".into(),
                PropertyValue::String(directive.rationale.clone()),
            );
            properties.insert(
                "updated_at".into(),
                PropertyValue::Integer(directive.updated_at as i64),
            );
            nodes.push(GraphNodeRecord {
                id: format!("directive:{}", directive.task_key),
                labels: vec!["Directive".into(), "RoutingDirective".into()],
                properties,
            });
            if let Some(preferred_agent) = directive.preferred_agent {
                relationships.push(GraphRelationshipRecord {
                    id: format!("directive-agent-{}", directive.task_key),
                    start_node: format!("directive:{}", directive.task_key),
                    end_node: agent_node_id(preferred_agent),
                    rel_type: "ROUTES_TO".into(),
                    properties: HashMap::new(),
                });
            }
        }

        for shift in &self.stigmergic_memory.policy_shifts {
            let mut properties = HashMap::new();
            properties.insert("policy_id".into(), PropertyValue::String(shift.id.clone()));
            properties.insert(
                "category".into(),
                PropertyValue::String(shift.category.clone()),
            );
            if let Some(target_agent) = shift.target_agent {
                properties.insert(
                    "target_agent".into(),
                    PropertyValue::Integer(target_agent as i64),
                );
            }
            if let Some(target_task) = &shift.target_task {
                properties.insert(
                    "target_task".into(),
                    PropertyValue::String(target_task.clone()),
                );
            }
            properties.insert("value".into(), PropertyValue::String(shift.value.clone()));
            properties.insert("confidence".into(), PropertyValue::Float(shift.confidence));
            properties.insert(
                "rationale".into(),
                PropertyValue::String(shift.rationale.clone()),
            );
            properties.insert(
                "updated_at".into(),
                PropertyValue::Integer(shift.updated_at as i64),
            );
            nodes.push(GraphNodeRecord {
                id: format!("policy:{}", shift.id),
                labels: vec!["PolicyShift".into()],
                properties,
            });
        }

        for template in &self.fact_templates {
            let mut properties = HashMap::new();
            properties.insert(
                "template_id".into(),
                PropertyValue::String(template.id.clone()),
            );
            properties.insert(
                "label".into(),
                PropertyValue::String(template.label.clone()),
            );
            properties.insert(
                "narrative".into(),
                PropertyValue::String(template.narrative.clone()),
            );
            properties.insert(
                "slot_names".into(),
                PropertyValue::StringList(template.slot_names.clone()),
            );
            properties.insert(
                "created_at".into(),
                PropertyValue::Integer(template.created_at as i64),
            );
            properties.insert(
                "updated_at".into(),
                PropertyValue::Integer(template.updated_at as i64),
            );
            nodes.push(GraphNodeRecord {
                id: fact_template_node_id(&template.id),
                labels: vec!["FactTemplate".into()],
                properties,
            });
        }

        for fact in &self.composite_facts {
            let mut properties = HashMap::new();
            properties.insert("fact_id".into(), PropertyValue::String(fact.id.clone()));
            properties.insert(
                "fact_kind".into(),
                PropertyValue::String(format!("{:?}", fact.kind)),
            );
            properties.insert("label".into(), PropertyValue::String(fact.label.clone()));
            properties.insert(
                "details".into(),
                PropertyValue::String(fact.details.clone()),
            );
            if let Some(template_id) = &fact.template_id {
                properties.insert(
                    "template_id".into(),
                    PropertyValue::String(template_id.clone()),
                );
            }
            properties.insert(
                "slots".into(),
                PropertyValue::StringList(
                    fact.slots.iter().map(serialize_fact_slot_binding).collect(),
                ),
            );
            properties.insert(
                "discovered_at".into(),
                PropertyValue::Integer(fact.temporal.discovered_at as i64),
            );
            properties.insert(
                "created_at".into(),
                PropertyValue::Integer(fact.temporal.created_at as i64),
            );
            properties.insert(
                "updated_at".into(),
                PropertyValue::Integer(fact.temporal.updated_at as i64),
            );
            if let Some(valid_from) = fact.temporal.valid_from {
                properties.insert(
                    "valid_from".into(),
                    PropertyValue::Integer(valid_from as i64),
                );
            }
            if let Some(valid_until) = fact.temporal.valid_until {
                properties.insert(
                    "valid_until".into(),
                    PropertyValue::Integer(valid_until as i64),
                );
            }
            if let Some(occurred_at) = fact.temporal.occurred_at {
                properties.insert(
                    "occurred_at".into(),
                    PropertyValue::Integer(occurred_at as i64),
                );
            }
            properties.insert("confidence".into(), PropertyValue::Float(fact.confidence));
            properties.insert("tags".into(), PropertyValue::StringList(fact.tags.clone()));
            if let Some(external_ref) = &fact.external_ref {
                properties.insert(
                    "external_ref".into(),
                    PropertyValue::String(external_ref.clone()),
                );
            }
            nodes.push(GraphNodeRecord {
                id: fact_node_id(&fact.id),
                labels: vec!["CompositeFact".into(), format!("{:?}", fact.kind)],
                properties,
            });
            if let Some(template_id) = &fact.template_id {
                relationships.push(GraphRelationshipRecord {
                    id: format!("fact-template-{}", fact.id),
                    start_node: fact_node_id(&fact.id),
                    end_node: fact_template_node_id(template_id),
                    rel_type: "USES_TEMPLATE".into(),
                    properties: HashMap::new(),
                });
            }
            for (slot_idx, slot) in fact.slots.iter().enumerate() {
                if let Some(target_node) = slot.entity_ref.as_deref().and_then(graph_ref_to_node_id)
                {
                    let mut rel_props = HashMap::new();
                    rel_props.insert("role".into(), PropertyValue::String(slot.role.clone()));
                    rel_props.insert("value".into(), PropertyValue::String(slot.value.clone()));
                    relationships.push(GraphRelationshipRecord {
                        id: format!("fact-slot-{}-{}", fact.id, slot_idx),
                        start_node: fact_node_id(&fact.id),
                        end_node: target_node,
                        rel_type: "HAS_SLOT_REF".into(),
                        properties: rel_props,
                    });
                }
            }
        }

        for relation in &self.recursive_fact_relations {
            let mut properties = HashMap::new();
            properties.insert(
                "relation_id".into(),
                PropertyValue::String(relation.id.clone()),
            );
            properties.insert(
                "confidence".into(),
                PropertyValue::Float(relation.confidence),
            );
            properties.insert(
                "rationale".into(),
                PropertyValue::String(relation.rationale.clone()),
            );
            properties.insert(
                "created_at".into(),
                PropertyValue::Integer(relation.created_at as i64),
            );
            relationships.push(GraphRelationshipRecord {
                id: relation.id.clone(),
                start_node: fact_node_id(&relation.from_fact_id),
                end_node: fact_node_id(&relation.to_fact_id),
                rel_type: relation.kind.as_str().into(),
                properties,
            });
        }

        for frame in &self.delegation_frames {
            let mut properties = HashMap::new();
            properties.insert(
                "delegation_id".into(),
                PropertyValue::String(frame.id.clone()),
            );
            properties.insert(
                "task_key".into(),
                PropertyValue::String(frame.task_key.clone()),
            );
            if let Some(requester) = frame.requester {
                properties.insert("requester".into(), PropertyValue::Integer(requester as i64));
            }
            properties.insert(
                "delegated_to".into(),
                PropertyValue::Integer(frame.delegated_to as i64),
            );
            properties.insert(
                "rationale".into(),
                PropertyValue::String(frame.rationale.clone()),
            );
            properties.insert("confidence".into(), PropertyValue::Float(frame.confidence));
            if let Some(promise_id) = &frame.promise_id {
                properties.insert(
                    "promise_id".into(),
                    PropertyValue::String(promise_id.clone()),
                );
            }
            properties.insert(
                "status".into(),
                PropertyValue::String(format!("{:?}", frame.status)),
            );
            properties.insert(
                "created_at".into(),
                PropertyValue::Integer(frame.created_at as i64),
            );
            properties.insert(
                "updated_at".into(),
                PropertyValue::Integer(frame.updated_at as i64),
            );
            if let Some(outcome_fact_id) = &frame.outcome_fact_id {
                properties.insert(
                    "outcome_fact_id".into(),
                    PropertyValue::String(outcome_fact_id.clone()),
                );
            }
            nodes.push(GraphNodeRecord {
                id: delegation_node_id(&frame.id),
                labels: vec!["DelegationFrame".into()],
                properties,
            });
            if let Some(requester) = frame.requester {
                relationships.push(GraphRelationshipRecord {
                    id: format!("delegation-requester-{}", frame.id),
                    start_node: delegation_node_id(&frame.id),
                    end_node: agent_node_id(requester),
                    rel_type: "REQUESTED_BY".into(),
                    properties: HashMap::new(),
                });
            }
            relationships.push(GraphRelationshipRecord {
                id: format!("delegation-target-{}", frame.id),
                start_node: delegation_node_id(&frame.id),
                end_node: agent_node_id(frame.delegated_to),
                rel_type: "DELEGATED_TO".into(),
                properties: HashMap::new(),
            });
            if let Some(promise_id) = &frame.promise_id {
                if let Some(promise_fact) = self
                    .composite_facts
                    .iter()
                    .find(|fact| fact.external_ref.as_deref() == Some(promise_id.as_str()))
                {
                    relationships.push(GraphRelationshipRecord {
                        id: format!("delegation-promise-{}", frame.id),
                        start_node: delegation_node_id(&frame.id),
                        end_node: fact_node_id(&promise_fact.id),
                        rel_type: "TRACKS_PROMISE".into(),
                        properties: HashMap::new(),
                    });
                }
            }
            if let Some(outcome_fact_id) = &frame.outcome_fact_id {
                relationships.push(GraphRelationshipRecord {
                    id: format!("delegation-outcome-{}", frame.id),
                    start_node: delegation_node_id(&frame.id),
                    end_node: fact_node_id(outcome_fact_id),
                    rel_type: "HAS_OUTCOME".into(),
                    properties: HashMap::new(),
                });
            }
        }

        PropertyGraphSnapshot {
            nodes,
            relationships,
        }
    }
}

fn agent_node_id(agent_id: AgentId) -> String {
    format!("agent:{agent_id}")
}

fn vertex_node_id(vertex_idx: usize) -> String {
    format!("vertex:{vertex_idx}")
}

fn ontology_node_id(concept: &str) -> String {
    format!("ontology:{concept}")
}

fn fact_template_node_id(template_id: &str) -> String {
    format!("fact_template:{template_id}")
}

fn fact_node_id(fact_id: &str) -> String {
    format!("fact:{fact_id}")
}

fn delegation_node_id(delegation_id: &str) -> String {
    format!("delegation:{delegation_id}")
}

fn serialize_fact_slot_binding(binding: &crate::hyper_stigmergy::FactSlotBinding) -> String {
    format!(
        "{}|{}|{}",
        binding.role,
        binding.value,
        binding.entity_ref.clone().unwrap_or_default()
    )
}

fn graph_ref_to_node_id(reference: &str) -> Option<String> {
    if reference.starts_with("agent:")
        || reference.starts_with("fact:")
        || reference.starts_with("fact_template:")
        || reference.starts_with("delegation:")
        || reference.starts_with("trace:")
        || reference.starts_with("directive:")
        || reference.starts_with("policy:")
    {
        Some(reference.to_string())
    } else if reference.starts_with("promise:") {
        Some(reference.replacen("promise:", "fact:", 1))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_world_into_property_graph() {
        let world = HyperStigmergicMorphogenesis::new(3);
        let snapshot = world.to_property_graph_snapshot();
        assert!(!snapshot.nodes.is_empty());
        assert!(snapshot
            .nodes
            .iter()
            .any(|n| n.labels.iter().any(|l| l == "Agent")));
        assert!(snapshot
            .nodes
            .iter()
            .any(|n| n.labels.iter().any(|l| l == "Property")));
    }
}
