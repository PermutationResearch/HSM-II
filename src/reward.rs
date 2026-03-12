use crate::metrics::RewardSignal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TaskEvalContext {
    pub coherence_delta: f64,
    pub exec_ok: bool,
    pub task_score: Option<f64>,
    pub tests_passed: Option<bool>,
    pub ground_truth_score: Option<f64>,
    pub latency_penalty: Option<f64>,
}

impl TaskEvalContext {
    pub fn from_json_str(input: &str) -> Option<Self> {
        serde_json::from_str::<TaskEvalContext>(input).ok()
    }
}

pub trait TaskEvaluator: Send + Sync {
    fn evaluate(&self, context: &TaskEvalContext) -> RewardSignal;
}

#[derive(Clone, Debug, Default)]
pub struct DefaultTaskEvaluator;

impl TaskEvaluator for DefaultTaskEvaluator {
    fn evaluate(&self, context: &TaskEvalContext) -> RewardSignal {
        let exec_bonus = if context.exec_ok { 0.05 } else { -0.05 };
        let total = context.coherence_delta + exec_bonus;
        RewardSignal {
            coherence_delta: context.coherence_delta,
            exec_ok: context.exec_ok,
            exec_bonus,
            task_score: context.task_score,
            tests_passed: context.tests_passed,
            ground_truth_score: context.ground_truth_score,
            latency_penalty: context.latency_penalty,
            total,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RewardWeights {
    pub coherence_weight: f64,
    pub task_score_weight: f64,
    pub ground_truth_weight: f64,
    pub tests_weight: f64,
    pub exec_bonus: f64,
    pub latency_weight: f64,
}

impl Default for RewardWeights {
    fn default() -> Self {
        Self {
            coherence_weight: 1.0,
            task_score_weight: 0.0,
            ground_truth_weight: 0.0,
            tests_weight: 0.0,
            exec_bonus: 0.05,
            latency_weight: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WeightedTaskEvaluator {
    pub weights: RewardWeights,
}

impl TaskEvaluator for WeightedTaskEvaluator {
    fn evaluate(&self, context: &TaskEvalContext) -> RewardSignal {
        let exec_bonus = if context.exec_ok {
            self.weights.exec_bonus
        } else {
            -self.weights.exec_bonus
        };
        let tests_bonus = context
            .tests_passed
            .map(|ok| if ok { 1.0 } else { -1.0 })
            .unwrap_or(0.0);
        let latency_penalty = context.latency_penalty.unwrap_or(0.0);
        let task_score = context.task_score.unwrap_or(0.0);
        let ground_truth = context.ground_truth_score.unwrap_or(0.0);
        let total = (self.weights.coherence_weight * context.coherence_delta)
            + (self.weights.task_score_weight * task_score)
            + (self.weights.ground_truth_weight * ground_truth)
            + (self.weights.tests_weight * tests_bonus)
            + exec_bonus
            - (self.weights.latency_weight * latency_penalty);

        RewardSignal {
            coherence_delta: context.coherence_delta,
            exec_ok: context.exec_ok,
            exec_bonus,
            task_score: context.task_score,
            tests_passed: context.tests_passed,
            ground_truth_score: context.ground_truth_score,
            latency_penalty: context.latency_penalty,
            total,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DatasetTaskEvaluator {
    pub weights: RewardWeights,
}

impl TaskEvaluator for DatasetTaskEvaluator {
    fn evaluate(&self, context: &TaskEvalContext) -> RewardSignal {
        let evaluator = WeightedTaskEvaluator {
            weights: self.weights.clone(),
        };
        evaluator.evaluate(context)
    }
}
