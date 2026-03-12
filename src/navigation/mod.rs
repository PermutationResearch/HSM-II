//! Topic-modeled code navigation - "Browse by meaning" for codebases.
//!
//! Parses code into semantic units, clusters by topic, and provides
//! natural language code search.

use std::collections::HashMap;

pub mod indexer;
pub mod parser;
pub mod search;

pub use indexer::{CodeIndex, SemanticIndex, TopicModel};
pub use parser::{CodeParser, Language, ParsedUnit};
pub use search::{QueryIntent, SearchResult, SemanticSearch};

/// Code navigation system with semantic indexing
pub struct CodeNavigator {
    parser: CodeParser,
    index: CodeIndex,
    search_engine: SemanticSearch,
}

impl CodeNavigator {
    pub fn new() -> Self {
        Self {
            parser: CodeParser::new(),
            index: CodeIndex::new(),
            search_engine: SemanticSearch::new(),
        }
    }

    /// Index a codebase at the given path
    pub fn index_codebase(&mut self, root_path: &std::path::Path) -> anyhow::Result<IndexStats> {
        let mut total_units = 0;
        let mut total_files = 0;

        // Walk directory and parse files
        for entry in walkdir::WalkDir::new(root_path)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() {
                if let Some(lang) = detect_language(path) {
                    total_files += 1;

                    if let Ok(content) = std::fs::read_to_string(path) {
                        let units = self.parser.parse(&content, lang, path);
                        total_units += units.len();

                        for unit in units {
                            self.index.add_unit(unit);
                        }
                    }
                }
            }
        }

        // Build topic model
        self.index.build_topics(10); // 10 topics default

        // Initialize search engine
        self.search_engine.build(&self.index);

        Ok(IndexStats {
            total_files,
            total_units,
            total_topics: self.index.topic_count(),
        })
    }

    /// Search for code by natural language query
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        self.search_engine.search(query, &self.index, limit)
    }

    /// Browse code by topic
    pub fn browse_by_topic(&self, topic_id: TopicId) -> Vec<&ParsedUnit> {
        self.index.get_units_by_topic(topic_id)
    }

    /// Get related code units
    pub fn related_units(&self, unit_id: &str, limit: usize) -> Vec<&ParsedUnit> {
        self.index.get_related_units(unit_id, limit)
    }

    /// Get topic distribution for a file
    pub fn file_topics(&self, file_path: &std::path::Path) -> HashMap<TopicId, f64> {
        self.index.get_file_topic_distribution(file_path)
    }

    /// Lightweight index stats for telemetry.
    pub fn stats_snapshot(&self) -> IndexStats {
        IndexStats {
            total_files: 0, // file-level bookkeeping is internal to CodeIndex
            total_units: self.index.units().len(),
            total_topics: self.index.topic_count(),
        }
    }
}

/// Statistics from indexing operation
#[derive(Clone, Debug)]
pub struct IndexStats {
    pub total_files: usize,
    pub total_units: usize,
    pub total_topics: usize,
}

/// Topic identifier
pub type TopicId = usize;

/// Detect language from file path
fn detect_language(path: &std::path::Path) -> Option<Language> {
    let ext = path.extension()?.to_str()?;

    match ext {
        "rs" => Some(Language::Rust),
        "py" => Some(Language::Python),
        "js" | "ts" | "jsx" | "tsx" => Some(Language::JavaScript),
        "go" => Some(Language::Go),
        "java" => Some(Language::Java),
        "c" | "h" => Some(Language::C),
        "cpp" | "hpp" => Some(Language::Cpp),
        _ => None,
    }
}
