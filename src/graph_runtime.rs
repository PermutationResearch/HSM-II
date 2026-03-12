use crate::columnar_engine::ColumnarGraphStore;
use crate::external_connectors::{
    DuckDbCliConnector, ExternalConnector, JsonArrayConnector, PostgresCliConnector,
};
use crate::hnsw_index::HnswLikeIndex;
use crate::hyper_stigmergy::HyperStigmergicMorphogenesis;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphToolKind {
    CypherLikeQuery,
    ColumnarScan,
    VectorAnn,
    ExternalScan,
}

#[derive(Clone, Debug)]
pub struct GraphActionPlan {
    pub tool: GraphToolKind,
    pub rationale: String,
    pub rewritten_query: String,
}

#[derive(Clone, Debug)]
pub struct GraphActionResult {
    pub tool: GraphToolKind,
    pub summary: String,
}

pub struct GraphRuntime;

impl GraphRuntime {
    pub fn tool_name(tool: &GraphToolKind) -> &'static str {
        match tool {
            GraphToolKind::CypherLikeQuery => "cypher",
            GraphToolKind::ColumnarScan => "columnar",
            GraphToolKind::VectorAnn => "vector_ann",
            GraphToolKind::ExternalScan => "external",
        }
    }

    pub fn parse_tool_name(value: &str) -> Option<GraphToolKind> {
        match value {
            "cypher" => Some(GraphToolKind::CypherLikeQuery),
            "columnar" => Some(GraphToolKind::ColumnarScan),
            "vector_ann" => Some(GraphToolKind::VectorAnn),
            "external" => Some(GraphToolKind::ExternalScan),
            _ => None,
        }
    }

    pub fn plan(input: &str) -> GraphActionPlan {
        let q = input.to_lowercase();
        if q.contains("similar") || q.contains("nearest") || q.contains("embedding") {
            return GraphActionPlan {
                tool: GraphToolKind::VectorAnn,
                rationale: "Semantic similarity request mapped to ANN vector search".into(),
                rewritten_query: input.to_string(),
            };
        }
        if q.contains("scan json") || q.contains("scan file") || q.contains("external") {
            return GraphActionPlan {
                tool: GraphToolKind::ExternalScan,
                rationale: "External data request mapped to connector scan".into(),
                rewritten_query: input.to_string(),
            };
        }
        if q.contains("count") || q.contains("aggregate") || q.contains("column") {
            return GraphActionPlan {
                tool: GraphToolKind::ColumnarScan,
                rationale: "Aggregate-like request mapped to columnar scan".into(),
                rewritten_query: input.to_string(),
            };
        }
        GraphActionPlan {
            tool: GraphToolKind::CypherLikeQuery,
            rationale: "Defaulting to graph pattern query".into(),
            rewritten_query: natural_to_match_query(input),
        }
    }

    pub fn plan_with_preference(
        input: &str,
        preferred_tool: Option<GraphToolKind>,
    ) -> GraphActionPlan {
        if let Some(tool) = preferred_tool {
            let mut plan = Self::plan(input);
            plan.tool = tool;
            plan.rationale = format!("Stigmergic override; {}", plan.rationale);
            return plan;
        }
        Self::plan(input)
    }

    pub fn execute(world: &HyperStigmergicMorphogenesis, input: &str) -> GraphActionResult {
        let plan = Self::plan(input);
        Self::execute_plan(world, &plan)
    }

    pub fn execute_plan(
        world: &HyperStigmergicMorphogenesis,
        plan: &GraphActionPlan,
    ) -> GraphActionResult {
        match plan.tool {
            GraphToolKind::CypherLikeQuery => {
                let result = world.run_cypher_like_query(&plan.rewritten_query);
                GraphActionResult {
                    tool: plan.tool.clone(),
                    summary: format!(
                        "{} -> {} node(s), {} relationship(s)",
                        plan.rewritten_query,
                        result.nodes.len(),
                        result.relationships.len()
                    ),
                }
            }
            GraphToolKind::ColumnarScan => {
                let snapshot = world.property_graph_snapshot();
                let columns = ColumnarGraphStore::from_snapshot(&snapshot);
                let label = extract_label(&plan.rewritten_query).unwrap_or_else(|| "Agent".into());
                let hits = columns.scan_equals("nodes", "labels", &label);
                GraphActionResult {
                    tool: plan.tool.clone(),
                    summary: format!("Columnar scan on labels={} -> {} row(s)", label, hits.len()),
                }
            }
            GraphToolKind::VectorAnn => {
                let mut index = HnswLikeIndex::new(3, 6);
                for agent in &world.agents {
                    index.insert(
                        agent.id as usize,
                        vec![
                            agent.drives.curiosity as f32,
                            agent.drives.harmony as f32,
                            agent.jw as f32,
                        ],
                    );
                }
                let query = [1.0, 0.5, 1.0];
                let hits = index.search(&query, 3);
                GraphActionResult {
                    tool: plan.tool.clone(),
                    summary: format!("ANN search returned {} candidate(s)", hits.len()),
                }
            }
            GraphToolKind::ExternalScan => {
                if let Some((kind, target, query)) = extract_external_spec(&plan.rewritten_query) {
                    let scanned = match kind.as_str() {
                        "duckdb" => DuckDbCliConnector {
                            database_path: target,
                            query: query.unwrap_or_else(|| "SELECT 1".into()),
                            table_name: "duckdb_scan".into(),
                        }
                        .scan(),
                        "postgres" => PostgresCliConnector {
                            connection_string: target,
                            query: query.unwrap_or_else(|| "SELECT 1".into()),
                            table_name: "postgres_scan".into(),
                        }
                        .scan(),
                        _ => JsonArrayConnector {
                            path: target,
                            table_name: "external".into(),
                        }
                        .scan(),
                    };
                    match scanned {
                        Ok(table) => GraphActionResult {
                            tool: plan.tool.clone(),
                            summary: format!(
                                "External scan loaded {} row(s) with {} column(s)",
                                table.rows.len(),
                                table.columns.len()
                            ),
                        },
                        Err(err) => GraphActionResult {
                            tool: plan.tool.clone(),
                            summary: format!("External scan failed: {}", err),
                        },
                    }
                } else if let Some(path) = extract_path(&plan.rewritten_query) {
                    let connector = JsonArrayConnector {
                        path,
                        table_name: "external".into(),
                    };
                    match connector.scan() {
                        Ok(table) => GraphActionResult {
                            tool: plan.tool.clone(),
                            summary: format!(
                                "External scan loaded {} row(s) with {} column(s)",
                                table.rows.len(),
                                table.columns.len()
                            ),
                        },
                        Err(err) => GraphActionResult {
                            tool: plan.tool.clone(),
                            summary: format!("External scan failed: {}", err),
                        },
                    }
                } else {
                    GraphActionResult {
                        tool: plan.tool.clone(),
                        summary: "External scan requested but no path found".into(),
                    }
                }
            }
        }
    }
}

fn natural_to_match_query(input: &str) -> String {
    let lower = input.to_lowercase();
    if lower.contains("agent") {
        "MATCH (n:Agent) RETURN n LIMIT 10".into()
    } else if lower.contains("property") {
        "MATCH (n:Property) RETURN n LIMIT 10".into()
    } else if lower.contains("belief") {
        "MATCH (n:Belief) RETURN n LIMIT 10".into()
    } else {
        "MATCH (n) RETURN n LIMIT 10".into()
    }
}

fn extract_label(input: &str) -> Option<String> {
    ["Agent", "Property", "Belief", "Experience", "Ontology"]
        .iter()
        .find(|label| {
            input.contains(**label) || input.to_lowercase().contains(&label.to_lowercase())
        })
        .map(|label| (*label).to_string())
}

fn extract_path(input: &str) -> Option<String> {
    input
        .split_whitespace()
        .find(|token| token.ends_with(".json"))
        .map(|s| s.to_string())
}

fn extract_external_spec(input: &str) -> Option<(String, String, Option<String>)> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }
    if parts[0].eq_ignore_ascii_case("scan") && parts[1].eq_ignore_ascii_case("duckdb") {
        let query = input.split(" query ").nth(1).map(|s| s.to_string());
        return Some(("duckdb".into(), parts[2].to_string(), query));
    }
    if parts[0].eq_ignore_ascii_case("scan") && parts[1].eq_ignore_ascii_case("postgres") {
        let query = input.split(" query ").nth(1).map(|s| s.to_string());
        return Some(("postgres".into(), parts[2].to_string(), query));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planner_chooses_ann_for_similarity_queries() {
        let plan = GraphRuntime::plan("find similar agents by embedding");
        assert_eq!(plan.tool, GraphToolKind::VectorAnn);
    }

    #[test]
    fn planner_defaults_to_graph_query() {
        let plan = GraphRuntime::plan("list agent nodes");
        assert_eq!(plan.tool, GraphToolKind::CypherLikeQuery);
    }
}
