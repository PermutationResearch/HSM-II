use crate::cypher_parser::{CypherParser, CypherQuery, MatchClause, ReturnExpr};
use crate::property_graph::{
    GraphNodeRecord, GraphRelationshipRecord, PropertyGraphSnapshot, PropertyValue,
};

#[derive(Clone, Debug, Default)]
pub struct QueryResultSet {
    pub nodes: Vec<GraphNodeRecord>,
    pub relationships: Vec<GraphRelationshipRecord>,
}

pub struct CypherEngine;

impl CypherEngine {
    pub fn execute(snapshot: &PropertyGraphSnapshot, query: &str) -> QueryResultSet {
        let Some(parsed) = CypherParser::parse(query) else {
            return QueryResultSet::default();
        };
        Self::execute_parsed(snapshot, &parsed)
    }

    pub fn execute_parsed(snapshot: &PropertyGraphSnapshot, query: &CypherQuery) -> QueryResultSet {
        match (&query.match_clause, &query.return_expr) {
            (MatchClause::Node(pattern), ReturnExpr::Node(ret_var))
                if ret_var == &pattern.variable =>
            {
                let mut nodes: Vec<GraphNodeRecord> = snapshot
                    .nodes
                    .iter()
                    .filter(|node| {
                        let label_ok = pattern
                            .label
                            .as_ref()
                            .map(|label| node.labels.iter().any(|l| l == label))
                            .unwrap_or(true);
                        let where_ok = query
                            .where_clause
                            .as_ref()
                            .map(|where_clause| {
                                where_clause.variable == pattern.variable
                                    && node
                                        .properties
                                        .get(&where_clause.property)
                                        .map(|value| property_matches(value, &where_clause.value))
                                        .unwrap_or(false)
                            })
                            .unwrap_or(true);
                        label_ok && where_ok
                    })
                    .cloned()
                    .collect();
                if let Some(limit) = query.limit {
                    nodes.truncate(limit);
                }
                QueryResultSet {
                    nodes,
                    relationships: vec![],
                }
            }
            (MatchClause::Relationship(pattern), ReturnExpr::Relationship(ret_var))
                if ret_var == &pattern.variable =>
            {
                let mut relationships: Vec<GraphRelationshipRecord> = snapshot
                    .relationships
                    .iter()
                    .filter(|rel| {
                        let type_ok = pattern
                            .rel_type
                            .as_ref()
                            .map(|rel_type| &rel.rel_type == rel_type)
                            .unwrap_or(true);
                        let where_ok = query
                            .where_clause
                            .as_ref()
                            .map(|where_clause| {
                                where_clause.variable == pattern.variable
                                    && rel
                                        .properties
                                        .get(&where_clause.property)
                                        .map(|value| property_matches(value, &where_clause.value))
                                        .unwrap_or(false)
                            })
                            .unwrap_or(true);
                        type_ok && where_ok
                    })
                    .cloned()
                    .collect();
                if let Some(limit) = query.limit {
                    relationships.truncate(limit);
                }
                QueryResultSet {
                    nodes: vec![],
                    relationships,
                }
            }
            (MatchClause::Path(pattern), ReturnExpr::Relationship(ret_var))
                if ret_var == &pattern.relationship_variable =>
            {
                let mut relationships: Vec<GraphRelationshipRecord> = snapshot
                    .relationships
                    .iter()
                    .filter(|rel| path_relationship_matches(snapshot, pattern, rel))
                    .cloned()
                    .collect();
                if let Some(limit) = query.limit {
                    relationships.truncate(limit);
                }
                QueryResultSet {
                    nodes: vec![],
                    relationships,
                }
            }
            (MatchClause::Path(pattern), ReturnExpr::Node(ret_var))
                if ret_var == &pattern.start_variable || ret_var == &pattern.end_variable =>
            {
                let mut nodes: Vec<GraphNodeRecord> = snapshot
                    .relationships
                    .iter()
                    .filter(|rel| path_relationship_matches(snapshot, pattern, rel))
                    .filter_map(|rel| {
                        let node_id = if ret_var == &pattern.start_variable {
                            &rel.start_node
                        } else {
                            &rel.end_node
                        };
                        snapshot.find_node(node_id).cloned()
                    })
                    .collect();
                if let Some(limit) = query.limit {
                    nodes.truncate(limit);
                }
                QueryResultSet {
                    nodes,
                    relationships: vec![],
                }
            }
            _ => QueryResultSet::default(),
        }
    }
}

fn property_matches(value: &PropertyValue, expected: &str) -> bool {
    match value {
        PropertyValue::String(s) => s == expected,
        PropertyValue::Integer(i) => i.to_string() == expected,
        PropertyValue::Float(f) => f.to_string() == expected,
        PropertyValue::Boolean(b) => b.to_string() == expected,
        PropertyValue::StringList(items) => items.iter().any(|item| item == expected),
    }
}

fn path_relationship_matches(
    snapshot: &PropertyGraphSnapshot,
    pattern: &crate::cypher_parser::MatchPathPattern,
    rel: &GraphRelationshipRecord,
) -> bool {
    let type_ok = pattern
        .relationship_type
        .as_ref()
        .map(|rel_type| &rel.rel_type == rel_type)
        .unwrap_or(true);
    if !type_ok {
        return false;
    }
    let Some(start) = snapshot.find_node(&rel.start_node) else {
        return false;
    };
    let Some(end) = snapshot.find_node(&rel.end_node) else {
        return false;
    };
    let start_ok = pattern
        .start_label
        .as_ref()
        .map(|label| start.labels.iter().any(|l| l == label))
        .unwrap_or(true);
    let end_ok = pattern
        .end_label
        .as_ref()
        .map(|label| end.labels.iter().any(|l| l == label))
        .unwrap_or(true);
    start_ok && end_ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hyper_stigmergy::HyperStigmergicMorphogenesis;

    #[test]
    fn executes_parsed_node_query() {
        let mut world = HyperStigmergicMorphogenesis::new(3);
        world.agents[0].description = "alpha".into();
        let snapshot = world.to_property_graph_snapshot();
        let results = CypherEngine::execute(
            &snapshot,
            "MATCH (n:Agent) WHERE n.description = 'alpha' RETURN n LIMIT 1",
        );
        assert_eq!(results.nodes.len(), 1);
    }

    #[test]
    fn executes_relationship_query() {
        let mut world = HyperStigmergicMorphogenesis::new(2);
        world.tick();
        let snapshot = world.to_property_graph_snapshot();
        let results = CypherEngine::execute(&snapshot, "MATCH ()-[r:HYPEREDGE_LINK]-() RETURN r");
        assert!(results.relationships.len() <= snapshot.relationships.len());
    }

    #[test]
    fn executes_path_relationship_query() {
        let mut world = HyperStigmergicMorphogenesis::new(2);
        world.edges.push(crate::hyper_stigmergy::HyperEdge {
            participants: vec![0, 1],
            weight: 1.0,
            emergent: false,
            age: 0,
            tags: std::collections::HashMap::new(),
            created_at: 0,
            embedding: None,
            scope: None,
            provenance: None,
            trust_tags: None,
            origin_system: None,
            knowledge_layer: None,
        });
        let snapshot = world.to_property_graph_snapshot();
        let results = CypherEngine::execute(
            &snapshot,
            "MATCH (a:Agent)-[r:HYPEREDGE_LINK]->(b:Agent) RETURN r LIMIT 1",
        );
        assert_eq!(results.relationships.len(), 1);
    }
}
