//! Named eval suites and weighted multi-suite specs (transfer-style objectives).

use super::tasks::{
    load_eval_suite, suite_council_vs_single, suite_memory_retrieval, suite_tool_routing, EvalTask,
};

/// Resolve a single pre-registered suite by name.
pub fn eval_tasks_for_suite(name: &str) -> Result<Vec<EvalTask>, String> {
    match name.trim() {
        "memory" => Ok(suite_memory_retrieval()),
        "tool" | "tools" => Ok(suite_tool_routing()),
        "council" => Ok(suite_council_vs_single()),
        "full" => Ok(load_eval_suite()),
        other => Err(format!(
            "unknown suite {:?}; use full | memory | tool | council",
            other
        )),
    }
}

/// One weighted slice of tasks (e.g. for multi-suite transfer objectives).
#[derive(Clone, Debug)]
pub struct WeightedEvalSuite {
    pub name: String,
    pub weight: f64,
    pub tasks: Vec<EvalTask>,
}

/// Parse `memory:1,tool:0.5,council:1` or `memory,tool` (implicit weight 1.0 each).
pub fn parse_weighted_suites(spec: &str) -> Result<Vec<WeightedEvalSuite>, String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("empty --suites".into());
    }
    let mut out = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (name, weight) = if let Some((n, w)) = part.split_once(':') {
            let w = w
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("invalid weight in suite spec fragment {:?}", part))?;
            if w < 0.0 {
                return Err(format!("negative weight in {:?}", part));
            }
            (n.trim().to_string(), w)
        } else {
            (part.to_string(), 1.0)
        };
        if name.is_empty() {
            return Err(format!("empty suite name in {:?}", part));
        }
        let tasks = eval_tasks_for_suite(&name)?;
        out.push(WeightedEvalSuite {
            name,
            weight,
            tasks,
        });
    }
    if out.is_empty() {
        Err("no suites parsed".into())
    } else {
        Ok(out)
    }
}

/// Apply optional task id prefix filter (comma-separated) and limit, mutating each suite's tasks.
pub fn filter_tasks(tasks: &mut Vec<EvalTask>, filter: Option<&str>, limit: Option<usize>) {
    if let Some(f) = filter {
        let prefixes: Vec<&str> = f.split(',').map(|s| s.trim()).collect();
        tasks.retain(|t| prefixes.iter().any(|p| t.id.starts_with(p)));
    }
    if let Some(n) = limit {
        tasks.truncate(n);
    }
}
