use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReturnExpr {
    Node(String),
    Relationship(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchNodePattern {
    pub variable: String,
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchRelationshipPattern {
    pub variable: String,
    pub rel_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchPathPattern {
    pub start_variable: String,
    pub start_label: Option<String>,
    pub relationship_variable: String,
    pub relationship_type: Option<String>,
    pub end_variable: String,
    pub end_label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchClause {
    Node(MatchNodePattern),
    Relationship(MatchRelationshipPattern),
    Path(MatchPathPattern),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhereClause {
    pub variable: String,
    pub property: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CypherQuery {
    pub match_clause: MatchClause,
    pub where_clause: Option<WhereClause>,
    pub return_expr: ReturnExpr,
    pub limit: Option<usize>,
}

pub struct CypherParser;

impl CypherParser {
    pub fn parse(query: &str) -> Option<CypherQuery> {
        let normalized = query.trim();
        let match_body = normalized.strip_prefix("MATCH ")?;
        let (match_part, rest) = split_once_keyword(match_body, "RETURN")?;
        let where_split = split_once_keyword(match_part, "WHERE");
        let (pattern_str, where_clause) = match where_split {
            Some((pattern, where_text)) => {
                (pattern.trim(), Some(Self::parse_where(where_text.trim())?))
            }
            None => (match_part.trim(), None),
        };

        let match_clause = if pattern_str.contains("]->(") {
            // Directed path: (a)-[r]->(b)
            Self::parse_path_match(pattern_str)?
        } else if pattern_str.starts_with("()-[") {
            // Undirected relationship: ()-[r]-()
            Self::parse_relationship_match(pattern_str)?
        } else if pattern_str.starts_with('(') && !pattern_str.contains(")-[") {
            Self::parse_node_match(pattern_str)?
        } else {
            return None;
        };

        let (return_part, limit) =
            if let Some((ret, lim)) = split_once_keyword(rest.trim(), "LIMIT") {
                (ret.trim(), lim.trim().parse::<usize>().ok())
            } else {
                (rest.trim(), None)
            };

        let return_expr = Self::parse_return(return_part)?;
        Some(CypherQuery {
            match_clause,
            where_clause,
            return_expr,
            limit,
        })
    }

    fn parse_node_match(input: &str) -> Option<MatchClause> {
        let inner = input.strip_prefix('(')?.strip_suffix(')')?;
        let mut parts = inner.split(':');
        let variable = parts.next()?.trim().to_string();
        let label = parts
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Some(MatchClause::Node(MatchNodePattern { variable, label }))
    }

    fn parse_relationship_match(input: &str) -> Option<MatchClause> {
        let inner = input.strip_prefix("()-[")?.strip_suffix("]-()")?;
        let mut parts = inner.split(':');
        let variable = parts.next()?.trim().to_string();
        let rel_type = parts
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Some(MatchClause::Relationship(MatchRelationshipPattern {
            variable,
            rel_type,
        }))
    }

    fn parse_path_match(input: &str) -> Option<MatchClause> {
        let (left, rest) = input.split_once(")-[")?;
        let left = left.strip_prefix('(')?;
        let (rel_part, right) = rest.split_once("]->(")?;
        let right = right.strip_suffix(')')?;

        let mut left_parts = left.split(':');
        let start_variable = left_parts.next()?.trim().to_string();
        let start_label = left_parts
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let mut rel_parts = rel_part.split(':');
        let relationship_variable = rel_parts.next()?.trim().to_string();
        let relationship_type = rel_parts
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let mut right_parts = right.split(':');
        let end_variable = right_parts.next()?.trim().to_string();
        let end_label = right_parts
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        Some(MatchClause::Path(MatchPathPattern {
            start_variable,
            start_label,
            relationship_variable,
            relationship_type,
            end_variable,
            end_label,
        }))
    }

    fn parse_where(input: &str) -> Option<WhereClause> {
        let (lhs, rhs) = input.split_once('=')?;
        let lhs = lhs.trim();
        let (variable, property) = lhs.split_once('.')?;
        Some(WhereClause {
            variable: variable.trim().to_string(),
            property: property.trim().to_string(),
            value: rhs.trim().trim_matches('\'').trim_matches('"').to_string(),
        })
    }

    fn parse_return(input: &str) -> Option<ReturnExpr> {
        let variable = input.trim().strip_prefix("RETURN ").unwrap_or(input.trim());
        if variable.is_empty() {
            return None;
        }
        Some(if variable.starts_with('r') {
            ReturnExpr::Relationship(variable.to_string())
        } else {
            ReturnExpr::Node(variable.to_string())
        })
    }
}

fn split_once_keyword<'a>(input: &'a str, keyword: &str) -> Option<(&'a str, &'a str)> {
    let needle = format!(" {} ", keyword);
    if let Some(idx) = input.find(&needle) {
        let left = &input[..idx];
        let right = &input[idx + needle.len()..];
        Some((left, right))
    } else if input.starts_with(&(keyword.to_string() + " ")) {
        Some(("", &input[keyword.len() + 1..]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_node_match_where_return_limit() {
        let q = CypherParser::parse("MATCH (n:Agent) WHERE n.role = 'Architect' RETURN n LIMIT 2")
            .unwrap();
        assert!(matches!(q.match_clause, MatchClause::Node(_)));
        assert_eq!(q.limit, Some(2));
    }

    #[test]
    fn parses_relationship_match() {
        let q = CypherParser::parse("MATCH ()-[r:HYPEREDGE_LINK]-() RETURN r").unwrap();
        assert!(matches!(q.match_clause, MatchClause::Relationship(_)));
    }

    #[test]
    fn parses_path_match() {
        let q =
            CypherParser::parse("MATCH (a:Agent)-[r:HYPEREDGE_LINK]->(b:Agent) RETURN r LIMIT 3")
                .unwrap();
        assert!(matches!(q.match_clause, MatchClause::Path(_)));
        assert_eq!(q.limit, Some(3));
    }
}
