use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::process::Command;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalRow {
    pub values: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalTable {
    pub name: String,
    pub columns: Vec<String>,
    pub rows: Vec<ExternalRow>,
}

pub trait ExternalConnector {
    fn scan(&self) -> anyhow::Result<ExternalTable>;
}

pub struct JsonArrayConnector {
    pub path: String,
    pub table_name: String,
}

impl ExternalConnector for JsonArrayConnector {
    fn scan(&self) -> anyhow::Result<ExternalTable> {
        let text = fs::read_to_string(&self.path)?;
        table_from_json_array(&text, &self.table_name)
    }
}

pub struct DuckDbCliConnector {
    pub database_path: String,
    pub query: String,
    pub table_name: String,
}

impl ExternalConnector for DuckDbCliConnector {
    fn scan(&self) -> anyhow::Result<ExternalTable> {
        let output = Command::new("duckdb")
            .arg(&self.database_path)
            .arg("-json")
            .arg(&self.query)
            .output()?;
        if !output.status.success() {
            anyhow::bail!(
                "duckdb scan failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        table_from_json_array(&String::from_utf8_lossy(&output.stdout), &self.table_name)
    }
}

pub struct PostgresCliConnector {
    pub connection_string: String,
    pub query: String,
    pub table_name: String,
}

impl ExternalConnector for PostgresCliConnector {
    fn scan(&self) -> anyhow::Result<ExternalTable> {
        let output = Command::new("psql")
            .arg(&self.connection_string)
            .arg("-c")
            .arg(format!("COPY ({}) TO STDOUT WITH CSV HEADER", self.query))
            .output()?;
        if !output.status.success() {
            anyhow::bail!(
                "psql scan failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        table_from_csv(&String::from_utf8_lossy(&output.stdout), &self.table_name)
    }
}

fn table_from_json_array(text: &str, table_name: &str) -> anyhow::Result<ExternalTable> {
    let json: Value = serde_json::from_str(text)?;
    let items = json.as_array().cloned().unwrap_or_default();
    let mut columns = Vec::new();
    let mut rows = Vec::new();

    for item in items {
        let mut values = HashMap::new();
        if let Some(obj) = item.as_object() {
            for (key, value) in obj {
                if !columns.contains(key) {
                    columns.push(key.clone());
                }
                values.insert(key.clone(), stringify_json(value));
            }
        }
        rows.push(ExternalRow { values });
    }

    Ok(ExternalTable {
        name: table_name.to_string(),
        columns,
        rows,
    })
}

fn table_from_csv(text: &str, table_name: &str) -> anyhow::Result<ExternalTable> {
    let mut lines = text.lines();
    let headers: Vec<String> = lines
        .next()
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    let mut rows = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let mut values = HashMap::new();
        for (header, value) in headers.iter().zip(line.split(',')) {
            values.insert(header.clone(), value.trim().to_string());
        }
        rows.push(ExternalRow { values });
    }
    Ok(ExternalTable {
        name: table_name.to_string(),
        columns: headers,
        rows,
    })
}

fn stringify_json(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_array_connector_scans_rows() {
        let path = format!("/tmp/hsmii-connector-{}.json", std::process::id());
        fs::write(&path, r#"[{"name":"alex","role":"agent"}]"#).unwrap();
        let connector = JsonArrayConnector {
            path: path.clone(),
            table_name: "agents".into(),
        };
        let table = connector.scan().unwrap();
        assert_eq!(table.rows.len(), 1);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn csv_parser_scans_rows() {
        let table = table_from_csv("name,role\nalex,agent\n", "agents").unwrap();
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.columns.len(), 2);
    }
}
