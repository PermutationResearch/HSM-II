//! Personal-agent tools: `read_operations`, `list_tickets` (see `personal::ops_config`).

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::personal::ops_config::{load_ops_config, resolve_ops_config_path, OperationsConfig};

use super::{object_schema, Tool, ToolOutput, ToolRegistry};

const READ_NAME: &str = "read_operations";
const LIST_NAME: &str = "list_tickets";

/// Register ops tools bound to a specific agent home (`HSMII_HOME`).
pub fn register_personal_ops_tools(registry: &mut ToolRegistry, home: &std::path::Path) {
    let home = home.to_path_buf();
    registry.register(Arc::new(ReadOperationsTool { home: home.clone() }));
    registry.register(Arc::new(ListTicketsTool { home }));
}

struct ReadOperationsTool {
    home: PathBuf,
}

struct ListTicketsTool {
    home: PathBuf,
}

#[async_trait]
impl Tool for ReadOperationsTool {
    fn name(&self) -> &str {
        READ_NAME
    }

    fn description(&self) -> &str {
        "Load and validate enterprise operations YAML (goals, org, budgets, governance, heartbeats, optional tickets). Uses HSM_OPERATIONS_YAML or <HSMII_HOME>/config/operations.yaml. Returns JSON."
    }

    fn parameters_schema(&self) -> Value {
        let mut s = object_schema(vec![
            (
                "include_tickets",
                "If true (default), include the tickets array in the response.",
                false,
            ),
            (
                "path",
                "Optional absolute path to operations.yaml (overrides env and default for this call only).",
                false,
            ),
        ]);
        if let Value::Object(ref mut m) = s {
            m.get_mut("properties")
                .and_then(|p| p.as_object_mut())
                .and_then(|p| p.get_mut("include_tickets"))
                .map(|v| {
                    *v = json!({
                        "type": "boolean",
                        "description": "If true (default), include tickets in output."
                    });
                });
        }
        s
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let path = param_path(&params).unwrap_or_else(|| resolve_ops_config_path(&self.home));
        let include_tickets = params
            .get("include_tickets")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return ToolOutput::error(format!(
                "operations config not found: {} (set HSM_OPERATIONS_YAML or create {})",
                path.display(),
                path.display()
            ));
        }

        let p = path.clone();
        let loaded = tokio::task::spawn_blocking(move || -> anyhow::Result<OperationsConfig> {
            let cfg = load_ops_config(&p)?;
            cfg.validate()?;
            Ok(cfg)
        })
        .await;

        let Ok(Ok(cfg)) = loaded else {
            let msg = match loaded {
                Ok(Err(e)) => e.to_string(),
                Err(e) => e.to_string(),
                _ => "unknown error".into(),
            };
            return ToolOutput::error(msg);
        };

        let body = if include_tickets {
            match serde_json::to_value(&cfg) {
                Ok(v) => v,
                Err(e) => return ToolOutput::error(e.to_string()),
            }
        } else {
            cfg.summary_without_tickets()
        };

        ToolOutput::success(
            serde_json::to_string_pretty(&json!({
                "path": path.to_string_lossy(),
                "config": body,
            }))
            .unwrap_or_else(|_| "{}".into()),
        )
    }
}

#[async_trait]
impl Tool for ListTicketsTool {
    fn name(&self) -> &str {
        LIST_NAME
    }

    fn description(&self) -> &str {
        "List tickets from operations YAML. Optional filter by state (substring match, case-insensitive)."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            (
                "state",
                "Optional: only tickets whose state contains this string (case-insensitive).",
                false,
            ),
            (
                "path",
                "Optional absolute path to operations.yaml.",
                false,
            ),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let path = param_path(&params).unwrap_or_else(|| resolve_ops_config_path(&self.home));
        let state_filter = params
            .get("state")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());

        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return ToolOutput::error(format!(
                "operations config not found: {}",
                path.display()
            ));
        }

        let p = path.clone();
        let loaded = tokio::task::spawn_blocking(move || -> anyhow::Result<OperationsConfig> {
            let cfg = load_ops_config(&p)?;
            cfg.validate()?;
            Ok(cfg)
        })
        .await;

        let Ok(Ok(cfg)) = loaded else {
            let msg = match loaded {
                Ok(Err(e)) => e.to_string(),
                Err(e) => e.to_string(),
                _ => "unknown error".into(),
            };
            return ToolOutput::error(msg);
        };

        let tickets: Vec<_> = cfg
            .tickets
            .iter()
            .filter(|t| {
                state_filter.as_ref().map_or(true, |f| {
                    t.state.to_lowercase().contains(f.as_str())
                })
            })
            .cloned()
            .collect();

        let out = json!({
            "path": path.to_string_lossy(),
            "count": tickets.len(),
            "tickets": tickets,
        });

        ToolOutput::success(
            serde_json::to_string_pretty(&out).unwrap_or_else(|_| "[]".into()),
        )
    }
}

fn param_path(params: &Value) -> Option<PathBuf> {
    params
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}
