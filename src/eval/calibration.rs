//! Optional gold labels for rubric calibration (human or oracle LLM).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::metrics::TurnMetrics;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GoldFile {
    pub labels: Vec<GoldTurnLabel>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GoldTurnLabel {
    pub task_id: String,
    pub turn_index: usize,
    /// Expected rubric pass (ground truth for agreement stats).
    pub rubric_pass: bool,
}

pub fn load_gold_labels(path: &Path) -> anyhow::Result<HashMap<(String, usize), bool>> {
    let text = fs::read_to_string(path)?;
    let g: GoldFile = serde_json::from_str(&text)?;
    let mut m = HashMap::new();
    for row in g.labels {
        m.insert((row.task_id, row.turn_index), row.rubric_pass);
    }
    Ok(m)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalibrationReport {
    pub labeled_turns: usize,
    pub agreement_with_gold: f64,
    pub true_positive: usize,
    pub true_negative: usize,
    pub false_positive: usize,
    pub false_negative: usize,
}

/// Compare measured `TurnMetrics::rubric_pass` to gold on labeled (task, turn) keys only.
pub fn calibration_report(
    turns: &[TurnMetrics],
    gold: &HashMap<(String, usize), bool>,
) -> CalibrationReport {
    let mut tp = 0usize;
    let mut tn = 0usize;
    let mut fp = 0usize;
    let mut fn_ = 0usize;
    let mut labeled = 0usize;

    for t in turns {
        let key = (t.task_id.clone(), t.turn_index);
        let Some(&g_pass) = gold.get(&key) else {
            continue;
        };
        labeled += 1;
        let pred = t.rubric_pass;
        match (pred, g_pass) {
            (true, true) => tp += 1,
            (false, false) => tn += 1,
            (true, false) => fp += 1,
            (false, true) => fn_ += 1,
        }
    }

    let agreement = if labeled == 0 {
        0.0
    } else {
        (tp + tn) as f64 / labeled as f64
    };

    CalibrationReport {
        labeled_turns: labeled,
        agreement_with_gold: agreement,
        true_positive: tp,
        true_negative: tn,
        false_positive: fp,
        false_negative: fn_,
    }
}
