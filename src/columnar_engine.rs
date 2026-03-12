use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::property_graph::{PropertyGraphSnapshot, PropertyValue};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ColumnarTable {
    pub name: String,
    pub columns: HashMap<String, Vec<String>>,
    pub row_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ColumnarGraphStore {
    pub node_table: ColumnarTable,
    pub relationship_table: ColumnarTable,
}

impl ColumnarGraphStore {
    pub fn from_snapshot(snapshot: &PropertyGraphSnapshot) -> Self {
        let mut node_columns: HashMap<String, Vec<String>> = HashMap::new();
        let mut rel_columns: HashMap<String, Vec<String>> = HashMap::new();

        for node in &snapshot.nodes {
            push_col(&mut node_columns, "id", node.id.clone());
            push_col(&mut node_columns, "labels", node.labels.join("|"));
            for (key, value) in &node.properties {
                push_col(&mut node_columns, key, property_to_string(value));
            }
        }

        for rel in &snapshot.relationships {
            push_col(&mut rel_columns, "id", rel.id.clone());
            push_col(&mut rel_columns, "start_node", rel.start_node.clone());
            push_col(&mut rel_columns, "end_node", rel.end_node.clone());
            push_col(&mut rel_columns, "rel_type", rel.rel_type.clone());
            for (key, value) in &rel.properties {
                push_col(&mut rel_columns, key, property_to_string(value));
            }
        }

        Self {
            node_table: ColumnarTable {
                name: "nodes".into(),
                columns: node_columns,
                row_count: snapshot.nodes.len(),
            },
            relationship_table: ColumnarTable {
                name: "relationships".into(),
                columns: rel_columns,
                row_count: snapshot.relationships.len(),
            },
        }
    }

    pub fn scan_equals(&self, table: &str, column: &str, expected: &str) -> Vec<usize> {
        let table = match table {
            "nodes" => &self.node_table,
            "relationships" => &self.relationship_table,
            _ => return Vec::new(),
        };
        table
            .columns
            .get(column)
            .map(|values| {
                values
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, value)| (value == expected).then_some(idx))
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn push_col(columns: &mut HashMap<String, Vec<String>>, key: &str, value: String) {
    columns.entry(key.to_string()).or_default().push(value);
}

fn property_to_string(value: &PropertyValue) -> String {
    match value {
        PropertyValue::String(s) => s.clone(),
        PropertyValue::Integer(i) => i.to_string(),
        PropertyValue::Float(f) => f.to_string(),
        PropertyValue::Boolean(b) => b.to_string(),
        PropertyValue::StringList(items) => items.join("|"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hyper_stigmergy::HyperStigmergicMorphogenesis;

    #[test]
    fn builds_columnar_projection() {
        let world = HyperStigmergicMorphogenesis::new(2);
        let snapshot = world.to_property_graph_snapshot();
        let store = ColumnarGraphStore::from_snapshot(&snapshot);
        assert!(store.node_table.row_count > 0);
        assert!(store.node_table.columns.contains_key("id"));
    }
}
