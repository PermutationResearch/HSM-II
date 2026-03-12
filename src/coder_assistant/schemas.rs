//! TypeBox-style Schema Definitions for Tools
//!
//! Provides structured validation and serialization for tool parameters

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tool schema definition (TypeBox-style)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: ObjectSchema,
    pub required: Vec<String>,
}

/// Object schema for tool parameters
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    pub properties: HashMap<String, ParameterSchema>,
}

/// Individual parameter schema
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParameterSchema {
    #[serde(rename = "type")]
    pub param_type: ParameterType,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterType {
    String,
    Integer,
    Number,
    Boolean,
    Array,
    Object,
}

/// Tool parameter value
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolParameter {
    String(String),
    Integer(i64),
    Number(f64),
    Boolean(bool),
    Array(Vec<serde_json::Value>),
    Object(serde_json::Map<String, serde_json::Value>),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolProviderKind {
    Builtin,
    Mcp,
    Plugin,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WasmCapability {
    ReadWorkspace,
    WriteWorkspace,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolProviderRuntime {
    Http,
    Wasm {
        module_path: String,
        #[serde(default = "default_wasm_entrypoint")]
        entrypoint: String,
        #[serde(default)]
        capabilities: Vec<WasmCapability>,
    },
}

fn default_wasm_entrypoint() -> String {
    "run".to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolProviderMetadata {
    pub id: String,
    pub kind: ToolProviderKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<ToolProviderRuntime>,
}

/// Tool registry with schema validation
#[derive(Clone, Debug)]
pub struct ToolRegistry {
    tools: HashMap<String, ToolSchema>,
    providers: HashMap<String, ToolProviderMetadata>,
    tool_providers: HashMap<String, String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            tools: HashMap::new(),
            providers: HashMap::new(),
            tool_providers: HashMap::new(),
        };
        registry.register_provider(ToolProviderMetadata {
            id: "builtin".to_string(),
            kind: ToolProviderKind::Builtin,
            endpoint: None,
            version: None,
            capabilities: vec!["local-tools".to_string()],
            runtime: None,
        });
        registry.register_default_tools();
        registry
    }

    pub fn register(&mut self, schema: ToolSchema) {
        let tool_name = schema.name.clone();
        self.tools.insert(tool_name.clone(), schema);
        self.tool_providers
            .entry(tool_name)
            .or_insert_with(|| "builtin".to_string());
    }

    pub fn register_provider(&mut self, provider: ToolProviderMetadata) {
        self.providers.insert(provider.id.clone(), provider);
    }

    pub fn register_mcp_provider(
        &mut self,
        provider_id: impl Into<String>,
        endpoint: impl Into<String>,
        capabilities: Vec<String>,
    ) {
        self.register_provider(ToolProviderMetadata {
            id: provider_id.into(),
            kind: ToolProviderKind::Mcp,
            endpoint: Some(endpoint.into()),
            version: None,
            capabilities,
            runtime: Some(ToolProviderRuntime::Http),
        });
    }

    pub fn register_plugin_provider(
        &mut self,
        provider_id: impl Into<String>,
        version: Option<String>,
        capabilities: Vec<String>,
    ) {
        self.register_provider(ToolProviderMetadata {
            id: provider_id.into(),
            kind: ToolProviderKind::Plugin,
            endpoint: None,
            version,
            capabilities,
            runtime: None,
        });
    }

    pub fn register_wasm_plugin_provider(
        &mut self,
        provider_id: impl Into<String>,
        module_path: impl Into<String>,
        capabilities: Vec<String>,
        wasm_capabilities: Vec<WasmCapability>,
    ) {
        self.register_provider(ToolProviderMetadata {
            id: provider_id.into(),
            kind: ToolProviderKind::Plugin,
            endpoint: None,
            version: None,
            capabilities,
            runtime: Some(ToolProviderRuntime::Wasm {
                module_path: module_path.into(),
                entrypoint: default_wasm_entrypoint(),
                capabilities: wasm_capabilities,
            }),
        });
    }

    pub fn register_external_tool(
        &mut self,
        schema: ToolSchema,
        provider_id: &str,
    ) -> Result<(), ValidationError> {
        if !self.providers.contains_key(provider_id) {
            return Err(ValidationError::UnknownProvider(provider_id.to_string()));
        }
        let tool_name = schema.name.clone();
        self.tools.insert(tool_name.clone(), schema);
        self.tool_providers
            .insert(tool_name, provider_id.to_string());
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&ToolSchema> {
        self.tools.get(name)
    }

    pub fn list(&self) -> Vec<&ToolSchema> {
        self.tools.values().collect()
    }

    pub fn providers(&self) -> Vec<&ToolProviderMetadata> {
        self.providers.values().collect()
    }

    pub fn provider_for(&self, tool_name: &str) -> Option<&ToolProviderMetadata> {
        let provider_id = self.tool_providers.get(tool_name)?;
        self.providers.get(provider_id)
    }

    /// Validate tool arguments against schema
    pub fn validate(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<(), ValidationError> {
        let schema = self
            .tools
            .get(tool_name)
            .ok_or_else(|| ValidationError::UnknownTool(tool_name.to_string()))?;

        let obj = args.as_object().ok_or(ValidationError::InvalidArguments(
            "Expected object".to_string(),
        ))?;

        // Check required parameters
        for req in &schema.required {
            if !obj.contains_key(req) {
                return Err(ValidationError::MissingParameter(req.clone()));
            }
        }

        // Validate parameter types
        for (key, value) in obj {
            if let Some(prop) = schema.parameters.properties.get(key) {
                Self::validate_type(key, value, &prop.param_type)?;
            }
        }

        Ok(())
    }

    fn validate_type(
        name: &str,
        value: &serde_json::Value,
        expected: &ParameterType,
    ) -> Result<(), ValidationError> {
        let valid = match expected {
            ParameterType::String => value.is_string(),
            ParameterType::Integer => value.is_i64() || value.is_u64(),
            ParameterType::Number => value.is_number(),
            ParameterType::Boolean => value.is_boolean(),
            ParameterType::Array => value.is_array(),
            ParameterType::Object => value.is_object(),
        };

        if !valid {
            return Err(ValidationError::TypeMismatch {
                parameter: name.to_string(),
                expected: format!("{:?}", expected),
                got: format!("{:?}", value),
            });
        }

        Ok(())
    }

    fn register_default_tools(&mut self) {
        // Read tool
        self.register(ToolSchema {
            name: "read".to_string(),
            description: "Read file contents. Supports text files and images.".to_string(),
            parameters: ObjectSchema {
                schema_type: "object".to_string(),
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "path".to_string(),
                        ParameterSchema {
                            param_type: ParameterType::String,
                            description: "Path to the file (relative or absolute)".to_string(),
                            default: None,
                            enum_values: None,
                        },
                    );
                    props.insert(
                        "offset".to_string(),
                        ParameterSchema {
                            param_type: ParameterType::Integer,
                            description: "Line number to start from (1-indexed)".to_string(),
                            default: Some(serde_json::json!(1)),
                            enum_values: None,
                        },
                    );
                    props.insert(
                        "limit".to_string(),
                        ParameterSchema {
                            param_type: ParameterType::Integer,
                            description: "Maximum lines to read".to_string(),
                            default: Some(serde_json::json!(2000)),
                            enum_values: None,
                        },
                    );
                    props
                },
            },
            required: vec!["path".to_string()],
        });

        // Write tool
        self.register(ToolSchema {
            name: "write".to_string(),
            description: "Write content to a file. Creates parent directories if needed."
                .to_string(),
            parameters: ObjectSchema {
                schema_type: "object".to_string(),
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "path".to_string(),
                        ParameterSchema {
                            param_type: ParameterType::String,
                            description: "Path to the file".to_string(),
                            default: None,
                            enum_values: None,
                        },
                    );
                    props.insert(
                        "content".to_string(),
                        ParameterSchema {
                            param_type: ParameterType::String,
                            description: "Content to write".to_string(),
                            default: None,
                            enum_values: None,
                        },
                    );
                    props
                },
            },
            required: vec!["path".to_string(), "content".to_string()],
        });

        // Edit tool
        self.register(ToolSchema {
            name: "edit".to_string(),
            description: "Edit a file by replacing exact text. oldText must match exactly."
                .to_string(),
            parameters: ObjectSchema {
                schema_type: "object".to_string(),
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "path".to_string(),
                        ParameterSchema {
                            param_type: ParameterType::String,
                            description: "Path to the file".to_string(),
                            default: None,
                            enum_values: None,
                        },
                    );
                    props.insert(
                        "oldText".to_string(),
                        ParameterSchema {
                            param_type: ParameterType::String,
                            description: "Exact text to find and replace".to_string(),
                            default: None,
                            enum_values: None,
                        },
                    );
                    props.insert(
                        "newText".to_string(),
                        ParameterSchema {
                            param_type: ParameterType::String,
                            description: "New text to replace with".to_string(),
                            default: None,
                            enum_values: None,
                        },
                    );
                    props
                },
            },
            required: vec![
                "path".to_string(),
                "oldText".to_string(),
                "newText".to_string(),
            ],
        });

        // Bash tool
        self.register(ToolSchema {
            name: "bash".to_string(),
            description: "Execute a bash command in the current working directory.".to_string(),
            parameters: ObjectSchema {
                schema_type: "object".to_string(),
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "command".to_string(),
                        ParameterSchema {
                            param_type: ParameterType::String,
                            description: "Bash command to execute".to_string(),
                            default: None,
                            enum_values: None,
                        },
                    );
                    props.insert(
                        "timeout".to_string(),
                        ParameterSchema {
                            param_type: ParameterType::Integer,
                            description: "Timeout in seconds".to_string(),
                            default: None,
                            enum_values: None,
                        },
                    );
                    props
                },
            },
            required: vec!["command".to_string()],
        });

        // Grep tool
        self.register(ToolSchema {
            name: "grep".to_string(),
            description: "Search for pattern in files.".to_string(),
            parameters: ObjectSchema {
                schema_type: "object".to_string(),
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "pattern".to_string(),
                        ParameterSchema {
                            param_type: ParameterType::String,
                            description: "Pattern to search for".to_string(),
                            default: None,
                            enum_values: None,
                        },
                    );
                    props.insert(
                        "path".to_string(),
                        ParameterSchema {
                            param_type: ParameterType::String,
                            description: "Directory or file to search in".to_string(),
                            default: Some(serde_json::json!(".")),
                            enum_values: None,
                        },
                    );
                    props
                },
            },
            required: vec!["pattern".to_string()],
        });

        // Find tool (glob pattern search)
        self.register(ToolSchema {
            name: "find".to_string(),
            description: "Find files matching a glob-like pattern.".to_string(),
            parameters: ObjectSchema {
                schema_type: "object".to_string(),
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "pattern".to_string(),
                        ParameterSchema {
                            param_type: ParameterType::String,
                            description: "File pattern to search (e.g. *.rs)".to_string(),
                            default: None,
                            enum_values: None,
                        },
                    );
                    props
                },
            },
            required: vec!["pattern".to_string()],
        });

        // LS tool
        self.register(ToolSchema {
            name: "ls".to_string(),
            description: "List files/directories in the target path.".to_string(),
            parameters: ObjectSchema {
                schema_type: "object".to_string(),
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "path".to_string(),
                        ParameterSchema {
                            param_type: ParameterType::String,
                            description: "Path to list (defaults to .)".to_string(),
                            default: Some(serde_json::json!(".")),
                            enum_values: None,
                        },
                    );
                    props
                },
            },
            required: vec![],
        });
    }

    /// Convert to OpenAI-compatible function definitions
    pub fn to_openai_functions(&self) -> Vec<serde_json::Value> {
        self.tools
            .values()
            .map(|tool| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                })
            })
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub enum ValidationError {
    UnknownTool(String),
    UnknownProvider(String),
    MissingParameter(String),
    TypeMismatch {
        parameter: String,
        expected: String,
        got: String,
    },
    InvalidArguments(String),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::UnknownTool(name) => write!(f, "Unknown tool: {}", name),
            ValidationError::UnknownProvider(name) => write!(f, "Unknown tool provider: {}", name),
            ValidationError::MissingParameter(name) => {
                write!(f, "Missing required parameter: {}", name)
            }
            ValidationError::TypeMismatch {
                parameter,
                expected,
                got,
            } => {
                write!(
                    f,
                    "Type mismatch for {}: expected {}, got {}",
                    parameter, expected, got
                )
            }
            ValidationError::InvalidArguments(msg) => write!(f, "Invalid arguments: {}", msg),
        }
    }
}

impl std::error::Error for ValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_mcp_provider_and_external_tool() {
        let mut registry = ToolRegistry::new();
        registry.register_mcp_provider(
            "mailbox-mcp",
            "http://127.0.0.1:3000/mcp",
            vec!["mail".into(), "search".into()],
        );
        let schema = ToolSchema {
            name: "mail_search".to_string(),
            description: "Search external mailbox".to_string(),
            parameters: ObjectSchema {
                schema_type: "object".to_string(),
                properties: HashMap::new(),
            },
            required: vec![],
        };

        registry
            .register_external_tool(schema, "mailbox-mcp")
            .expect("provider should exist");

        let provider = registry
            .provider_for("mail_search")
            .expect("tool should be mapped to provider");
        assert_eq!(provider.kind, ToolProviderKind::Mcp);
        assert_eq!(
            provider.endpoint.as_deref(),
            Some("http://127.0.0.1:3000/mcp")
        );
    }
}
