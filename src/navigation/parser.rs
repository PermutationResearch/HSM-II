//! Code parser for extracting semantic units.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Supported programming languages
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    Go,
    Java,
    C,
    Cpp,
}

/// A semantic unit of code (function, struct, trait, etc.)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedUnit {
    pub id: String,
    pub name: String,
    pub unit_type: UnitType,
    pub language: Language,
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub content: String,
    pub documentation: Option<String>,
    pub signature: String,
    pub dependencies: Vec<String>,
    /// Semantic embedding (computed later)
    pub embedding: Option<Vec<f32>>,
    /// Topic assignment (computed during indexing)
    pub topic: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum UnitType {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Module,
    Class,
    Method,
    Variable,
    Constant,
    TypeAlias,
}

/// Code parser for extracting semantic units
pub struct CodeParser {
    // In production, would use tree-sitter or similar
}

impl CodeParser {
    pub fn new() -> Self {
        Self {}
    }

    /// Parse code content into semantic units
    pub fn parse(&self, content: &str, language: Language, file_path: &Path) -> Vec<ParsedUnit> {
        match language {
            Language::Rust => self.parse_rust(content, file_path),
            Language::Python => self.parse_python(content, file_path),
            Language::JavaScript => self.parse_javascript(content, file_path),
            Language::Go => self.parse_go(content, file_path),
            _ => self.parse_generic(content, file_path, language),
        }
    }

    fn parse_rust(&self, content: &str, file_path: &Path) -> Vec<ParsedUnit> {
        let mut units = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        let mut current_doc = String::new();
        let mut line_num = 0;

        for line in &lines {
            line_num += 1;
            let trimmed = line.trim();

            // Collect doc comments
            if trimmed.starts_with("///") || trimmed.starts_with("//!") {
                current_doc.push_str(trimmed);
                current_doc.push('\n');
                continue;
            }

            // Parse function
            if trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ") {
                if let Some(unit) =
                    self.extract_rust_function(trimmed, line_num, &current_doc, file_path, content)
                {
                    units.push(unit);
                }
                current_doc.clear();
            }

            // Parse struct
            if trimmed.starts_with("pub struct ") || trimmed.starts_with("struct ") {
                if let Some(unit) =
                    self.extract_rust_struct(trimmed, line_num, &current_doc, file_path)
                {
                    units.push(unit);
                }
                current_doc.clear();
            }

            // Parse trait
            if trimmed.starts_with("pub trait ") || trimmed.starts_with("trait ") {
                if let Some(unit) =
                    self.extract_rust_trait(trimmed, line_num, &current_doc, file_path)
                {
                    units.push(unit);
                }
                current_doc.clear();
            }

            // Parse impl
            if trimmed.starts_with("impl ") {
                if let Some(unit) = self.extract_rust_impl(trimmed, line_num, file_path) {
                    units.push(unit);
                }
            }
        }

        units
    }

    fn parse_python(&self, content: &str, file_path: &Path) -> Vec<ParsedUnit> {
        let mut units = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        let mut current_doc = String::new();
        let mut in_docstring = false;
        let mut line_num = 0;

        for line in &lines {
            line_num += 1;
            let trimmed = line.trim();

            // Docstring handling
            if trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''") {
                in_docstring = !in_docstring;
                if !in_docstring {
                    current_doc.clear();
                }
                continue;
            }

            if in_docstring {
                current_doc.push_str(trimmed);
                current_doc.push('\n');
                continue;
            }

            // Parse function
            if trimmed.starts_with("def ") {
                if let Some(unit) =
                    self.extract_python_function(trimmed, line_num, &current_doc, file_path)
                {
                    units.push(unit);
                }
                current_doc.clear();
            }

            // Parse class
            if trimmed.starts_with("class ") {
                if let Some(unit) =
                    self.extract_python_class(trimmed, line_num, &current_doc, file_path)
                {
                    units.push(unit);
                }
                current_doc.clear();
            }
        }

        units
    }

    fn parse_javascript(&self, content: &str, file_path: &Path) -> Vec<ParsedUnit> {
        let mut units = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        let mut line_num = 0;
        for line in &lines {
            line_num += 1;
            let trimmed = line.trim();

            // Parse function declarations
            if trimmed.starts_with("function ")
                || trimmed.contains("= function")
                || trimmed.contains("= ()")
                || trimmed.contains("=> {")
            {
                if let Some(unit) = self.extract_js_function(trimmed, line_num, file_path) {
                    units.push(unit);
                }
            }

            // Parse class
            if trimmed.starts_with("class ") {
                if let Some(unit) = self.extract_js_class(trimmed, line_num, file_path) {
                    units.push(unit);
                }
            }
        }

        units
    }

    fn parse_go(&self, content: &str, file_path: &Path) -> Vec<ParsedUnit> {
        let mut units = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        let mut current_doc = String::new();
        let mut line_num = 0;

        for line in &lines {
            line_num += 1;
            let trimmed = line.trim();

            // Go doc comments
            if trimmed.starts_with("// ") && !trimmed.starts_with("//go:") {
                current_doc.push_str(&trimmed[3..]);
                current_doc.push('\n');
                continue;
            }

            // Parse function
            if trimmed.starts_with("func ") {
                if let Some(unit) =
                    self.extract_go_function(trimmed, line_num, &current_doc, file_path)
                {
                    units.push(unit);
                }
                current_doc.clear();
            }

            // Parse type
            if trimmed.starts_with("type ") {
                if let Some(unit) = self.extract_go_type(trimmed, line_num, &current_doc, file_path)
                {
                    units.push(unit);
                }
                current_doc.clear();
            }
        }

        units
    }

    fn parse_generic(
        &self,
        _content: &str,
        _file_path: &Path,
        _language: Language,
    ) -> Vec<ParsedUnit> {
        // Fallback for unsupported languages
        Vec::new()
    }

    // Extract methods for each language
    fn extract_rust_function(
        &self,
        line: &str,
        line_num: usize,
        doc: &str,
        file_path: &Path,
        _content: &str,
    ) -> Option<ParsedUnit> {
        let name = extract_name(line, "fn ")?;
        Some(ParsedUnit {
            id: format!("{}:{}", file_path.display(), line_num),
            name,
            unit_type: UnitType::Function,
            language: Language::Rust,
            file_path: file_path.to_string_lossy().to_string(),
            line_start: line_num,
            line_end: line_num,
            content: line.to_string(),
            documentation: if doc.is_empty() {
                None
            } else {
                Some(doc.to_string())
            },
            signature: line.to_string(),
            dependencies: Vec::new(),
            embedding: None,
            topic: None,
        })
    }

    fn extract_rust_struct(
        &self,
        line: &str,
        line_num: usize,
        doc: &str,
        file_path: &Path,
    ) -> Option<ParsedUnit> {
        let name = extract_name(line, "struct ")?;
        Some(ParsedUnit {
            id: format!("{}:{}", file_path.display(), line_num),
            name,
            unit_type: UnitType::Struct,
            language: Language::Rust,
            file_path: file_path.to_string_lossy().to_string(),
            line_start: line_num,
            line_end: line_num,
            content: line.to_string(),
            documentation: if doc.is_empty() {
                None
            } else {
                Some(doc.to_string())
            },
            signature: line.to_string(),
            dependencies: Vec::new(),
            embedding: None,
            topic: None,
        })
    }

    fn extract_rust_trait(
        &self,
        line: &str,
        line_num: usize,
        doc: &str,
        file_path: &Path,
    ) -> Option<ParsedUnit> {
        let name = extract_name(line, "trait ")?;
        Some(ParsedUnit {
            id: format!("{}:{}", file_path.display(), line_num),
            name,
            unit_type: UnitType::Trait,
            language: Language::Rust,
            file_path: file_path.to_string_lossy().to_string(),
            line_start: line_num,
            line_end: line_num,
            content: line.to_string(),
            documentation: if doc.is_empty() {
                None
            } else {
                Some(doc.to_string())
            },
            signature: line.to_string(),
            dependencies: Vec::new(),
            embedding: None,
            topic: None,
        })
    }

    fn extract_rust_impl(
        &self,
        line: &str,
        line_num: usize,
        file_path: &Path,
    ) -> Option<ParsedUnit> {
        let name = line
            .split_whitespace()
            .nth(1)?
            .trim_end_matches('{')
            .trim()
            .to_string();
        Some(ParsedUnit {
            id: format!("{}:{}", file_path.display(), line_num),
            name,
            unit_type: UnitType::Impl,
            language: Language::Rust,
            file_path: file_path.to_string_lossy().to_string(),
            line_start: line_num,
            line_end: line_num,
            content: line.to_string(),
            documentation: None,
            signature: line.to_string(),
            dependencies: Vec::new(),
            embedding: None,
            topic: None,
        })
    }

    fn extract_python_function(
        &self,
        line: &str,
        line_num: usize,
        doc: &str,
        file_path: &Path,
    ) -> Option<ParsedUnit> {
        let name = extract_name(line, "def ")?;
        Some(ParsedUnit {
            id: format!("{}:{}", file_path.display(), line_num),
            name,
            unit_type: UnitType::Function,
            language: Language::Python,
            file_path: file_path.to_string_lossy().to_string(),
            line_start: line_num,
            line_end: line_num,
            content: line.to_string(),
            documentation: if doc.is_empty() {
                None
            } else {
                Some(doc.to_string())
            },
            signature: line.to_string(),
            dependencies: Vec::new(),
            embedding: None,
            topic: None,
        })
    }

    fn extract_python_class(
        &self,
        line: &str,
        line_num: usize,
        doc: &str,
        file_path: &Path,
    ) -> Option<ParsedUnit> {
        let name = extract_name(line, "class ")?;
        Some(ParsedUnit {
            id: format!("{}:{}", file_path.display(), line_num),
            name,
            unit_type: UnitType::Class,
            language: Language::Python,
            file_path: file_path.to_string_lossy().to_string(),
            line_start: line_num,
            line_end: line_num,
            content: line.to_string(),
            documentation: if doc.is_empty() {
                None
            } else {
                Some(doc.to_string())
            },
            signature: line.to_string(),
            dependencies: Vec::new(),
            embedding: None,
            topic: None,
        })
    }

    fn extract_js_function(
        &self,
        line: &str,
        line_num: usize,
        file_path: &Path,
    ) -> Option<ParsedUnit> {
        let name = if line.contains("function ") {
            extract_name(line, "function ")?
        } else {
            "anonymous".to_string()
        };

        Some(ParsedUnit {
            id: format!("{}:{}", file_path.display(), line_num),
            name,
            unit_type: UnitType::Function,
            language: Language::JavaScript,
            file_path: file_path.to_string_lossy().to_string(),
            line_start: line_num,
            line_end: line_num,
            content: line.to_string(),
            documentation: None,
            signature: line.to_string(),
            dependencies: Vec::new(),
            embedding: None,
            topic: None,
        })
    }

    fn extract_js_class(
        &self,
        line: &str,
        line_num: usize,
        file_path: &Path,
    ) -> Option<ParsedUnit> {
        let name = extract_name(line, "class ")?;
        Some(ParsedUnit {
            id: format!("{}:{}", file_path.display(), line_num),
            name,
            unit_type: UnitType::Class,
            language: Language::JavaScript,
            file_path: file_path.to_string_lossy().to_string(),
            line_start: line_num,
            line_end: line_num,
            content: line.to_string(),
            documentation: None,
            signature: line.to_string(),
            dependencies: Vec::new(),
            embedding: None,
            topic: None,
        })
    }

    fn extract_go_function(
        &self,
        line: &str,
        line_num: usize,
        doc: &str,
        file_path: &Path,
    ) -> Option<ParsedUnit> {
        let name = extract_name(line, "func ")?;
        Some(ParsedUnit {
            id: format!("{}:{}", file_path.display(), line_num),
            name,
            unit_type: UnitType::Function,
            language: Language::Go,
            file_path: file_path.to_string_lossy().to_string(),
            line_start: line_num,
            line_end: line_num,
            content: line.to_string(),
            documentation: if doc.is_empty() {
                None
            } else {
                Some(doc.to_string())
            },
            signature: line.to_string(),
            dependencies: Vec::new(),
            embedding: None,
            topic: None,
        })
    }

    fn extract_go_type(
        &self,
        line: &str,
        line_num: usize,
        doc: &str,
        file_path: &Path,
    ) -> Option<ParsedUnit> {
        let name = extract_name(line, "type ")?;
        Some(ParsedUnit {
            id: format!("{}:{}", file_path.display(), line_num),
            name,
            unit_type: UnitType::TypeAlias,
            language: Language::Go,
            file_path: file_path.to_string_lossy().to_string(),
            line_start: line_num,
            line_end: line_num,
            content: line.to_string(),
            documentation: if doc.is_empty() {
                None
            } else {
                Some(doc.to_string())
            },
            signature: line.to_string(),
            dependencies: Vec::new(),
            embedding: None,
            topic: None,
        })
    }
}

impl Default for CodeParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract name after a keyword
fn extract_name(line: &str, keyword: &str) -> Option<String> {
    let after = line.split(keyword).nth(1)?;
    let name = after
        .split(|c: char| c.is_whitespace() || c == '(' || c == '<' || c == '{')
        .next()?
        .trim()
        .to_string();

    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}
