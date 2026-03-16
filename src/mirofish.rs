//! MiroFish Trajectory Engine — Full trajectory mechanics for business decision support.
//!
//! Extends the basic scenario simulator with:
//! 1. Step-by-step action sequences (trajectory planning)
//! 2. Probability flow networks (Bayesian state transitions)
//! 3. Projection curves (time-series outcome modeling)
//! 4. Domain-specific scenario templates
//! 5. Confidence recalibration and back-testing
//! 6. Multi-turn refinement loops
//! 7. Variable engineering for business domains

use crate::ollama_client::OllamaClient;
use crate::scenario_simulator::{PredictionReport, ScenarioSimulatorConfig};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// 1. Step-by-step Action Sequences (Trajectory Planning)
// ─────────────────────────────────────────────────────────────────────────────

/// A single step in a trajectory — an action with expected outcome and timing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrajectoryStep {
    /// Step number (1-indexed)
    pub step: usize,
    /// Action to take
    pub action: String,
    /// Expected outcome of this action
    pub expected_outcome: String,
    /// Time horizon (e.g., "week 1", "month 3", "Q2 2026")
    pub time_horizon: String,
    /// Probability of success (0.0-1.0)
    pub success_probability: f64,
    /// Resources required
    pub resources: Vec<String>,
    /// Dependencies on previous steps
    pub depends_on: Vec<usize>,
    /// Risk factors specific to this step
    pub risks: Vec<String>,
}

/// A full trajectory — a sequence of steps from current state to target outcome.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trajectory {
    /// Trajectory name/label
    pub name: String,
    /// Starting state description
    pub initial_state: String,
    /// Target outcome
    pub target_outcome: String,
    /// Ordered steps
    pub steps: Vec<TrajectoryStep>,
    /// Overall trajectory probability (product of step probabilities, adjusted for dependencies)
    pub cumulative_probability: f64,
    /// Total estimated duration
    pub estimated_duration: String,
    /// Critical path (steps that cannot be parallelized)
    pub critical_path: Vec<usize>,
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Probability Flow Networks
// ─────────────────────────────────────────────────────────────────────────────

/// A state in the probability flow network
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlowState {
    /// State identifier
    pub id: String,
    /// Human-readable description
    pub description: String,
    /// Probability of being in this state
    pub probability: f64,
    /// Is this a terminal state?
    pub terminal: bool,
    /// Business impact score (-10 to +10)
    pub impact_score: f64,
}

/// A transition between states
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlowTransition {
    /// Source state ID
    pub from: String,
    /// Target state ID
    pub to: String,
    /// Transition probability
    pub probability: f64,
    /// Trigger condition
    pub trigger: String,
    /// Time to transition
    pub time_estimate: String,
}

/// Probability flow network — Bayesian state transition model
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbabilityFlowNetwork {
    /// All possible states
    pub states: Vec<FlowState>,
    /// Transitions between states
    pub transitions: Vec<FlowTransition>,
    /// Current state ID
    pub current_state: String,
    /// Time steps simulated
    pub time_steps: usize,
    /// Snapshot of state probabilities at each time step
    pub probability_history: Vec<HashMap<String, f64>>,
}

impl ProbabilityFlowNetwork {
    /// Create a new network from states and transitions
    pub fn new(states: Vec<FlowState>, transitions: Vec<FlowTransition>, current_state: &str) -> Self {
        Self {
            states,
            transitions,
            current_state: current_state.to_string(),
            time_steps: 0,
            probability_history: Vec::new(),
        }
    }

    /// Simulate one time step — propagate probabilities through transitions.
    /// Uses Bayesian update: P(state_t+1) = sum(P(state_t) * P(transition))
    pub fn step(&mut self) {
        let current_probs: HashMap<String, f64> = self
            .states
            .iter()
            .map(|s| (s.id.clone(), s.probability))
            .collect();

        let mut next_probs: HashMap<String, f64> = HashMap::new();

        // For each state, compute outgoing probability flow
        for state in &self.states {
            let state_prob = current_probs.get(&state.id).copied().unwrap_or(0.0);
            if state.terminal || state_prob < 1e-10 {
                // Terminal states retain their probability
                *next_probs.entry(state.id.clone()).or_insert(0.0) += state_prob;
                continue;
            }

            // Find all outgoing transitions from this state
            let outgoing: Vec<&FlowTransition> = self
                .transitions
                .iter()
                .filter(|t| t.from == state.id)
                .collect();

            if outgoing.is_empty() {
                // No transitions — state is implicitly terminal
                *next_probs.entry(state.id.clone()).or_insert(0.0) += state_prob;
                continue;
            }

            // Normalize transition probabilities (must sum to ≤ 1.0)
            let total_out: f64 = outgoing.iter().map(|t| t.probability).sum();
            let scale = if total_out > 1.0 {
                1.0 / total_out
            } else {
                1.0
            };

            // Distribute probability to target states
            let mut distributed = 0.0;
            for transition in &outgoing {
                let flow = state_prob * transition.probability * scale;
                *next_probs.entry(transition.to.clone()).or_insert(0.0) += flow;
                distributed += flow;
            }

            // Remaining probability stays in current state (didn't transition)
            let remaining = state_prob - distributed;
            if remaining > 1e-10 {
                *next_probs.entry(state.id.clone()).or_insert(0.0) += remaining;
            }
        }

        // Update state probabilities
        for state in &mut self.states {
            state.probability = next_probs.get(&state.id).copied().unwrap_or(0.0);
        }

        self.probability_history.push(next_probs);
        self.time_steps += 1;
    }

    /// Run N time steps of simulation
    pub fn simulate(&mut self, steps: usize) {
        for _ in 0..steps {
            self.step();
        }
    }

    /// Get the most likely terminal outcome
    pub fn most_likely_outcome(&self) -> Option<&FlowState> {
        self.states
            .iter()
            .filter(|s| s.terminal)
            .max_by(|a, b| a.probability.partial_cmp(&b.probability).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Get expected impact — weighted average of terminal state impacts
    pub fn expected_impact(&self) -> f64 {
        let terminal_states: Vec<&FlowState> = self.states.iter().filter(|s| s.terminal).collect();
        let total_prob: f64 = terminal_states.iter().map(|s| s.probability).sum();
        if total_prob < 1e-10 {
            return 0.0;
        }
        terminal_states
            .iter()
            .map(|s| s.probability * s.impact_score)
            .sum::<f64>()
            / total_prob
    }

    /// Get state probabilities as projection curve data points
    pub fn projection_curve(&self) -> Vec<ProjectionPoint> {
        let mut points = Vec::new();

        // Initial state
        let initial: HashMap<String, f64> = self
            .states
            .iter()
            .map(|s| (s.id.clone(), if s.id == self.current_state { 1.0 } else { 0.0 }))
            .collect();

        points.push(ProjectionPoint {
            time_step: 0,
            state_probabilities: initial,
            expected_impact: 0.0,
        });

        for (i, snapshot) in self.probability_history.iter().enumerate() {
            let impact: f64 = self
                .states
                .iter()
                .filter(|s| s.terminal)
                .map(|s| snapshot.get(&s.id).copied().unwrap_or(0.0) * s.impact_score)
                .sum();

            points.push(ProjectionPoint {
                time_step: i + 1,
                state_probabilities: snapshot.clone(),
                expected_impact: impact,
            });
        }

        points
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Projection Curves
// ─────────────────────────────────────────────────────────────────────────────

/// A single point in a projection curve
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectionPoint {
    /// Time step (0 = now)
    pub time_step: usize,
    /// Probability of each state at this time
    pub state_probabilities: HashMap<String, f64>,
    /// Expected impact at this time step
    pub expected_impact: f64,
}

/// A complete projection — the trajectory of probabilities over time
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectionCurve {
    /// Label for this projection
    pub label: String,
    /// Data points over time
    pub points: Vec<ProjectionPoint>,
    /// Confidence band (±)
    pub confidence_band: f64,
    /// Trend direction: "improving", "declining", "stable", "volatile"
    pub trend: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Domain-Specific Scenario Templates
// ─────────────────────────────────────────────────────────────────────────────

/// A scenario template for a specific business domain
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioTemplate {
    /// Template identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Business domain
    pub domain: ScenarioDomain,
    /// Description
    pub description: String,
    /// Required variables (user must provide)
    pub required_variables: Vec<VariableSpec>,
    /// Optional variables (have defaults)
    pub optional_variables: Vec<VariableSpec>,
    /// Pre-defined flow states for this template
    pub default_states: Vec<FlowState>,
    /// Pre-defined transitions
    pub default_transitions: Vec<FlowTransition>,
    /// Suggested number of time steps
    pub suggested_time_steps: usize,
    /// Suggested variants for scenario branches
    pub suggested_variants: Vec<String>,
}

/// Business domain categories
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ScenarioDomain {
    PricingStrategy,
    MarketEntry,
    ProductLaunch,
    CompetitiveResponse,
    GrowthPlanning,
    CostOptimization,
    HiringDecision,
    MarketingCampaign,
    FundraisingStrategy,
    Custom(String),
}

impl std::fmt::Display for ScenarioDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PricingStrategy => write!(f, "Pricing Strategy"),
            Self::MarketEntry => write!(f, "Market Entry"),
            Self::ProductLaunch => write!(f, "Product Launch"),
            Self::CompetitiveResponse => write!(f, "Competitive Response"),
            Self::GrowthPlanning => write!(f, "Growth Planning"),
            Self::CostOptimization => write!(f, "Cost Optimization"),
            Self::HiringDecision => write!(f, "Hiring Decision"),
            Self::MarketingCampaign => write!(f, "Marketing Campaign"),
            Self::FundraisingStrategy => write!(f, "Fundraising Strategy"),
            Self::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// Variable specification for templates
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VariableSpec {
    /// Variable name
    pub name: String,
    /// Description
    pub description: String,
    /// Example value
    pub example: String,
    /// Default value (if optional)
    pub default: Option<String>,
}

/// Get all built-in scenario templates
pub fn builtin_templates() -> Vec<ScenarioTemplate> {
    vec![
        pricing_strategy_template(),
        market_entry_template(),
        growth_planning_template(),
        marketing_campaign_template(),
        competitive_response_template(),
        cost_optimization_template(),
    ]
}

fn pricing_strategy_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "pricing_strategy".into(),
        name: "Pricing Strategy Decision".into(),
        domain: ScenarioDomain::PricingStrategy,
        description: "Evaluate pricing changes: increase, decrease, new tier, or freemium model".into(),
        required_variables: vec![
            VariableSpec { name: "current_price".into(), description: "Current price per unit/seat".into(), example: "$49/seat/month".into(), default: None },
            VariableSpec { name: "customer_count".into(), description: "Current paying customers".into(), example: "15".into(), default: None },
            VariableSpec { name: "current_mrr".into(), description: "Current monthly recurring revenue".into(), example: "$12,000".into(), default: None },
        ],
        optional_variables: vec![
            VariableSpec { name: "competitor_price".into(), description: "Main competitor's price".into(), example: "$39/seat/month".into(), default: Some("unknown".into()) },
            VariableSpec { name: "churn_rate".into(), description: "Monthly churn rate".into(), example: "5%".into(), default: Some("unknown".into()) },
            VariableSpec { name: "target_mrr".into(), description: "Target MRR".into(), example: "$50,000".into(), default: Some("2x current".into()) },
        ],
        default_states: vec![
            FlowState { id: "current".into(), description: "Current pricing maintained".into(), probability: 1.0, terminal: false, impact_score: 0.0 },
            FlowState { id: "price_increase_accepted".into(), description: "Customers accept price increase".into(), probability: 0.0, terminal: false, impact_score: 4.0 },
            FlowState { id: "price_increase_churn".into(), description: "Price increase triggers churn".into(), probability: 0.0, terminal: true, impact_score: -3.0 },
            FlowState { id: "new_tier_success".into(), description: "New pricing tier gains traction".into(), probability: 0.0, terminal: true, impact_score: 6.0 },
            FlowState { id: "new_tier_confusion".into(), description: "New tier confuses buyers".into(), probability: 0.0, terminal: true, impact_score: -2.0 },
            FlowState { id: "price_decrease_volume".into(), description: "Lower price drives volume".into(), probability: 0.0, terminal: true, impact_score: 3.0 },
            FlowState { id: "revenue_growth".into(), description: "Revenue grows sustainably".into(), probability: 0.0, terminal: true, impact_score: 8.0 },
        ],
        default_transitions: vec![
            FlowTransition { from: "current".into(), to: "price_increase_accepted".into(), probability: 0.4, trigger: "20% price increase announced".into(), time_estimate: "1 month".into() },
            FlowTransition { from: "current".into(), to: "price_increase_churn".into(), probability: 0.15, trigger: "Aggressive price increase".into(), time_estimate: "2 months".into() },
            FlowTransition { from: "current".into(), to: "new_tier_success".into(), probability: 0.25, trigger: "Enterprise tier launched".into(), time_estimate: "3 months".into() },
            FlowTransition { from: "current".into(), to: "new_tier_confusion".into(), probability: 0.1, trigger: "Too many pricing options".into(), time_estimate: "2 months".into() },
            FlowTransition { from: "current".into(), to: "price_decrease_volume".into(), probability: 0.1, trigger: "Price reduction for growth".into(), time_estimate: "2 months".into() },
            FlowTransition { from: "price_increase_accepted".into(), to: "revenue_growth".into(), probability: 0.7, trigger: "Customers see value".into(), time_estimate: "3 months".into() },
            FlowTransition { from: "price_increase_accepted".into(), to: "price_increase_churn".into(), probability: 0.3, trigger: "Delayed churn effect".into(), time_estimate: "4 months".into() },
        ],
        suggested_time_steps: 6,
        suggested_variants: vec!["conservative".into(), "aggressive".into(), "value-based".into(), "freemium".into()],
    }
}

fn market_entry_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "market_entry".into(),
        name: "New Market Entry".into(),
        domain: ScenarioDomain::MarketEntry,
        description: "Evaluate entering a new geographic or segment market".into(),
        required_variables: vec![
            VariableSpec { name: "target_market".into(), description: "Market to enter".into(), example: "European B2B SaaS".into(), default: None },
            VariableSpec { name: "budget".into(), description: "Budget for entry".into(), example: "$50,000".into(), default: None },
        ],
        optional_variables: vec![
            VariableSpec { name: "timeline".into(), description: "Target timeline".into(), example: "6 months".into(), default: Some("12 months".into()) },
            VariableSpec { name: "existing_presence".into(), description: "Any existing presence".into(), example: "3 customers via inbound".into(), default: Some("none".into()) },
        ],
        default_states: vec![
            FlowState { id: "pre_entry".into(), description: "Evaluating market".into(), probability: 1.0, terminal: false, impact_score: 0.0 },
            FlowState { id: "early_traction".into(), description: "First customers acquired".into(), probability: 0.0, terminal: false, impact_score: 3.0 },
            FlowState { id: "product_market_fit".into(), description: "Achieved PMF in new market".into(), probability: 0.0, terminal: true, impact_score: 9.0 },
            FlowState { id: "slow_growth".into(), description: "Growth below expectations".into(), probability: 0.0, terminal: false, impact_score: 1.0 },
            FlowState { id: "exit_market".into(), description: "Decided to exit market".into(), probability: 0.0, terminal: true, impact_score: -4.0 },
            FlowState { id: "pivot_segment".into(), description: "Pivoted to different segment".into(), probability: 0.0, terminal: true, impact_score: 2.0 },
        ],
        default_transitions: vec![
            FlowTransition { from: "pre_entry".into(), to: "early_traction".into(), probability: 0.5, trigger: "Launch marketing + sales".into(), time_estimate: "3 months".into() },
            FlowTransition { from: "pre_entry".into(), to: "slow_growth".into(), probability: 0.3, trigger: "Market resistance".into(), time_estimate: "3 months".into() },
            FlowTransition { from: "pre_entry".into(), to: "exit_market".into(), probability: 0.2, trigger: "Due diligence reveals blockers".into(), time_estimate: "1 month".into() },
            FlowTransition { from: "early_traction".into(), to: "product_market_fit".into(), probability: 0.6, trigger: "Strong retention signal".into(), time_estimate: "6 months".into() },
            FlowTransition { from: "early_traction".into(), to: "slow_growth".into(), probability: 0.4, trigger: "Churn or weak expansion".into(), time_estimate: "4 months".into() },
            FlowTransition { from: "slow_growth".into(), to: "pivot_segment".into(), probability: 0.4, trigger: "Identify better segment".into(), time_estimate: "3 months".into() },
            FlowTransition { from: "slow_growth".into(), to: "exit_market".into(), probability: 0.3, trigger: "Budget exhausted".into(), time_estimate: "6 months".into() },
            FlowTransition { from: "slow_growth".into(), to: "early_traction".into(), probability: 0.3, trigger: "Strategy adjustment works".into(), time_estimate: "3 months".into() },
        ],
        suggested_time_steps: 8,
        suggested_variants: vec!["direct-sales".into(), "partner-led".into(), "product-led".into(), "acquisition".into()],
    }
}

fn growth_planning_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "growth_planning".into(),
        name: "Growth Planning (Revenue Trajectory)".into(),
        domain: ScenarioDomain::GrowthPlanning,
        description: "Project revenue growth scenarios with different strategies".into(),
        required_variables: vec![
            VariableSpec { name: "current_mrr".into(), description: "Current MRR".into(), example: "$12,000".into(), default: None },
            VariableSpec { name: "target_mrr".into(), description: "Target MRR".into(), example: "$50,000".into(), default: None },
            VariableSpec { name: "timeline".into(), description: "Target timeline".into(), example: "12 months".into(), default: None },
        ],
        optional_variables: vec![
            VariableSpec { name: "growth_rate".into(), description: "Current monthly growth rate".into(), example: "8%".into(), default: Some("unknown".into()) },
            VariableSpec { name: "team_size".into(), description: "Team size".into(), example: "4".into(), default: Some("unknown".into()) },
        ],
        default_states: vec![
            FlowState { id: "current_growth".into(), description: "Maintaining current growth rate".into(), probability: 1.0, terminal: false, impact_score: 0.0 },
            FlowState { id: "accelerated".into(), description: "Growth accelerating above plan".into(), probability: 0.0, terminal: false, impact_score: 5.0 },
            FlowState { id: "plateau".into(), description: "Growth plateaued".into(), probability: 0.0, terminal: false, impact_score: -2.0 },
            FlowState { id: "target_hit".into(), description: "Revenue target achieved".into(), probability: 0.0, terminal: true, impact_score: 10.0 },
            FlowState { id: "target_missed".into(), description: "Target missed, need to reassess".into(), probability: 0.0, terminal: true, impact_score: -3.0 },
            FlowState { id: "exceeded".into(), description: "Exceeded target significantly".into(), probability: 0.0, terminal: true, impact_score: 10.0 },
        ],
        default_transitions: vec![
            FlowTransition { from: "current_growth".into(), to: "accelerated".into(), probability: 0.3, trigger: "New channel or product works".into(), time_estimate: "3 months".into() },
            FlowTransition { from: "current_growth".into(), to: "plateau".into(), probability: 0.3, trigger: "Market saturation or churn".into(), time_estimate: "4 months".into() },
            FlowTransition { from: "current_growth".into(), to: "target_hit".into(), probability: 0.2, trigger: "Steady execution".into(), time_estimate: "12 months".into() },
            FlowTransition { from: "current_growth".into(), to: "target_missed".into(), probability: 0.2, trigger: "External shock".into(), time_estimate: "12 months".into() },
            FlowTransition { from: "accelerated".into(), to: "exceeded".into(), probability: 0.5, trigger: "Viral growth or large deal".into(), time_estimate: "6 months".into() },
            FlowTransition { from: "accelerated".into(), to: "target_hit".into(), probability: 0.4, trigger: "Sustainable acceleration".into(), time_estimate: "8 months".into() },
            FlowTransition { from: "plateau".into(), to: "current_growth".into(), probability: 0.3, trigger: "New initiative".into(), time_estimate: "3 months".into() },
            FlowTransition { from: "plateau".into(), to: "target_missed".into(), probability: 0.4, trigger: "Can't break through".into(), time_estimate: "9 months".into() },
        ],
        suggested_time_steps: 12,
        suggested_variants: vec!["organic".into(), "paid-acquisition".into(), "product-led".into(), "enterprise-sales".into()],
    }
}

fn marketing_campaign_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "marketing_campaign".into(),
        name: "Marketing Campaign Outcomes".into(),
        domain: ScenarioDomain::MarketingCampaign,
        description: "Project outcomes of a marketing campaign or product launch".into(),
        required_variables: vec![
            VariableSpec { name: "campaign_type".into(), description: "Type of campaign".into(), example: "Product Hunt launch".into(), default: None },
            VariableSpec { name: "budget".into(), description: "Campaign budget".into(), example: "$5,000".into(), default: None },
            VariableSpec { name: "target_metric".into(), description: "Primary success metric".into(), example: "500 signups".into(), default: None },
        ],
        optional_variables: vec![
            VariableSpec { name: "audience_size".into(), description: "Estimated reach".into(), example: "50,000 developers".into(), default: Some("unknown".into()) },
            VariableSpec { name: "past_performance".into(), description: "Previous campaign results".into(), example: "2% conversion rate".into(), default: Some("no data".into()) },
        ],
        default_states: vec![
            FlowState { id: "pre_launch".into(), description: "Campaign prepared".into(), probability: 1.0, terminal: false, impact_score: 0.0 },
            FlowState { id: "viral".into(), description: "Campaign goes viral".into(), probability: 0.0, terminal: true, impact_score: 9.0 },
            FlowState { id: "strong_performance".into(), description: "Exceeds targets".into(), probability: 0.0, terminal: true, impact_score: 6.0 },
            FlowState { id: "meets_target".into(), description: "Hits expected targets".into(), probability: 0.0, terminal: true, impact_score: 3.0 },
            FlowState { id: "underperforms".into(), description: "Below expectations".into(), probability: 0.0, terminal: true, impact_score: -2.0 },
            FlowState { id: "flop".into(), description: "Campaign fails".into(), probability: 0.0, terminal: true, impact_score: -5.0 },
        ],
        default_transitions: vec![
            FlowTransition { from: "pre_launch".into(), to: "viral".into(), probability: 0.05, trigger: "Organic sharing takes off".into(), time_estimate: "1 week".into() },
            FlowTransition { from: "pre_launch".into(), to: "strong_performance".into(), probability: 0.2, trigger: "Good targeting + messaging".into(), time_estimate: "2 weeks".into() },
            FlowTransition { from: "pre_launch".into(), to: "meets_target".into(), probability: 0.35, trigger: "Solid execution".into(), time_estimate: "1 month".into() },
            FlowTransition { from: "pre_launch".into(), to: "underperforms".into(), probability: 0.3, trigger: "Audience fatigue or poor timing".into(), time_estimate: "1 month".into() },
            FlowTransition { from: "pre_launch".into(), to: "flop".into(), probability: 0.1, trigger: "Wrong channel or message".into(), time_estimate: "2 weeks".into() },
        ],
        suggested_time_steps: 4,
        suggested_variants: vec!["content-led".into(), "paid-ads".into(), "influencer".into(), "community-driven".into()],
    }
}

fn competitive_response_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "competitive_response".into(),
        name: "Competitive Response Planning".into(),
        domain: ScenarioDomain::CompetitiveResponse,
        description: "Plan response to competitor moves: price cuts, feature launches, acquisitions".into(),
        required_variables: vec![
            VariableSpec { name: "competitor_action".into(), description: "What the competitor did".into(), example: "Launched free tier".into(), default: None },
            VariableSpec { name: "our_position".into(), description: "Our current market position".into(), example: "Premium, 15 customers, $49/seat".into(), default: None },
        ],
        optional_variables: vec![
            VariableSpec { name: "our_advantage".into(), description: "Our key differentiator".into(), example: "3x faster, better accuracy".into(), default: Some("unknown".into()) },
            VariableSpec { name: "urgency".into(), description: "How urgent is response".into(), example: "losing deals to them".into(), default: Some("moderate".into()) },
        ],
        default_states: vec![
            FlowState { id: "status_quo".into(), description: "No response yet".into(), probability: 1.0, terminal: false, impact_score: -1.0 },
            FlowState { id: "match_price".into(), description: "Match competitor pricing".into(), probability: 0.0, terminal: true, impact_score: 1.0 },
            FlowState { id: "differentiate".into(), description: "Double down on differentiation".into(), probability: 0.0, terminal: true, impact_score: 5.0 },
            FlowState { id: "new_category".into(), description: "Create new category".into(), probability: 0.0, terminal: true, impact_score: 8.0 },
            FlowState { id: "lose_share".into(), description: "Lose market share".into(), probability: 0.0, terminal: true, impact_score: -6.0 },
        ],
        default_transitions: vec![
            FlowTransition { from: "status_quo".into(), to: "match_price".into(), probability: 0.2, trigger: "Price war decision".into(), time_estimate: "1 month".into() },
            FlowTransition { from: "status_quo".into(), to: "differentiate".into(), probability: 0.35, trigger: "Feature investment".into(), time_estimate: "3 months".into() },
            FlowTransition { from: "status_quo".into(), to: "new_category".into(), probability: 0.15, trigger: "Bold positioning shift".into(), time_estimate: "6 months".into() },
            FlowTransition { from: "status_quo".into(), to: "lose_share".into(), probability: 0.3, trigger: "No action taken".into(), time_estimate: "3 months".into() },
        ],
        suggested_time_steps: 6,
        suggested_variants: vec!["aggressive".into(), "defensive".into(), "flanking".into(), "ignore".into()],
    }
}

fn cost_optimization_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "cost_optimization".into(),
        name: "Cost Optimization Decision".into(),
        domain: ScenarioDomain::CostOptimization,
        description: "Evaluate cost reduction strategies: infra, team, tooling, vendor changes".into(),
        required_variables: vec![
            VariableSpec { name: "current_monthly_cost".into(), description: "Current monthly burn".into(), example: "$15,000".into(), default: None },
            VariableSpec { name: "cost_breakdown".into(), description: "Major cost categories".into(), example: "60% LLM APIs, 20% infra, 20% tools".into(), default: None },
            VariableSpec { name: "target_reduction".into(), description: "Target cost reduction".into(), example: "30%".into(), default: None },
        ],
        optional_variables: vec![
            VariableSpec { name: "constraints".into(), description: "Non-negotiable constraints".into(), example: "Cannot reduce quality".into(), default: Some("none".into()) },
        ],
        default_states: vec![
            FlowState { id: "current_costs".into(), description: "Current cost structure".into(), probability: 1.0, terminal: false, impact_score: 0.0 },
            FlowState { id: "optimized".into(), description: "Costs reduced, quality maintained".into(), probability: 0.0, terminal: true, impact_score: 7.0 },
            FlowState { id: "reduced_quality".into(), description: "Costs reduced but quality dropped".into(), probability: 0.0, terminal: true, impact_score: -3.0 },
            FlowState { id: "no_savings".into(), description: "Attempted but minimal savings".into(), probability: 0.0, terminal: true, impact_score: -1.0 },
            FlowState { id: "innovation_savings".into(), description: "Found innovative approach with major savings".into(), probability: 0.0, terminal: true, impact_score: 9.0 },
        ],
        default_transitions: vec![
            FlowTransition { from: "current_costs".into(), to: "optimized".into(), probability: 0.35, trigger: "Systematic optimization".into(), time_estimate: "2 months".into() },
            FlowTransition { from: "current_costs".into(), to: "reduced_quality".into(), probability: 0.2, trigger: "Aggressive cuts".into(), time_estimate: "1 month".into() },
            FlowTransition { from: "current_costs".into(), to: "no_savings".into(), probability: 0.3, trigger: "Already lean".into(), time_estimate: "2 months".into() },
            FlowTransition { from: "current_costs".into(), to: "innovation_savings".into(), probability: 0.15, trigger: "Architecture rethink".into(), time_estimate: "3 months".into() },
        ],
        suggested_time_steps: 4,
        suggested_variants: vec!["incremental".into(), "aggressive".into(), "strategic".into(), "innovation-driven".into()],
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Confidence Recalibration & Back-testing
// ─────────────────────────────────────────────────────────────────────────────

/// Historical prediction record for back-testing
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PredictionRecord {
    /// Unique ID
    pub id: String,
    /// When the prediction was made
    pub predicted_at: u64,
    /// The prediction topic
    pub topic: String,
    /// Predicted outcome
    pub predicted_outcome: String,
    /// Predicted confidence
    pub predicted_confidence: f64,
    /// Actual outcome (filled in later)
    pub actual_outcome: Option<String>,
    /// Did prediction come true? (filled in later)
    pub was_correct: Option<bool>,
    /// Calibration error = |predicted_confidence - actual_accuracy|
    pub calibration_error: Option<f64>,
}

/// Calibration statistics from back-testing
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalibrationStats {
    /// Total predictions evaluated
    pub total_evaluated: usize,
    /// Number correct
    pub correct: usize,
    /// Average predicted confidence
    pub avg_predicted_confidence: f64,
    /// Actual accuracy rate
    pub actual_accuracy: f64,
    /// Calibration error (lower = better calibrated)
    pub calibration_error: f64,
    /// Is the model overconfident or underconfident?
    pub direction: String,
    /// Suggested adjustment factor (multiply confidence by this)
    pub adjustment_factor: f64,
}

/// Compute calibration stats from prediction records
pub fn compute_calibration(records: &[PredictionRecord]) -> CalibrationStats {
    let evaluated: Vec<&PredictionRecord> = records
        .iter()
        .filter(|r| r.was_correct.is_some())
        .collect();

    if evaluated.is_empty() {
        return CalibrationStats {
            total_evaluated: 0,
            correct: 0,
            avg_predicted_confidence: 0.0,
            actual_accuracy: 0.0,
            calibration_error: 0.0,
            direction: "insufficient data".into(),
            adjustment_factor: 1.0,
        };
    }

    let correct = evaluated.iter().filter(|r| r.was_correct == Some(true)).count();
    let total = evaluated.len();
    let actual_accuracy = correct as f64 / total as f64;
    let avg_confidence: f64 = evaluated.iter().map(|r| r.predicted_confidence).sum::<f64>() / total as f64;
    let calibration_error = (avg_confidence - actual_accuracy).abs();

    let direction = if avg_confidence > actual_accuracy + 0.05 {
        "overconfident"
    } else if avg_confidence < actual_accuracy - 0.05 {
        "underconfident"
    } else {
        "well-calibrated"
    };

    let adjustment_factor = if avg_confidence > 0.01 {
        actual_accuracy / avg_confidence
    } else {
        1.0
    };

    CalibrationStats {
        total_evaluated: total,
        correct,
        avg_predicted_confidence: avg_confidence,
        actual_accuracy,
        calibration_error,
        direction: direction.into(),
        adjustment_factor: adjustment_factor.clamp(0.5, 2.0),
    }
}

/// Recalibrate a confidence score using calibration stats
pub fn recalibrate_confidence(raw_confidence: f64, stats: &CalibrationStats) -> f64 {
    if stats.total_evaluated < 5 {
        // Not enough data — return raw
        return raw_confidence;
    }
    (raw_confidence * stats.adjustment_factor).clamp(0.01, 0.99)
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Multi-Turn Refinement
// ─────────────────────────────────────────────────────────────────────────────

/// A refinement round — user provides feedback, model adjusts
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RefinementRound {
    /// Round number
    pub round: usize,
    /// User feedback or new information
    pub feedback: String,
    /// Adjustments made to the scenario
    pub adjustments: Vec<String>,
    /// Updated confidence after refinement
    pub updated_confidence: f64,
}

/// Refinement session state
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RefinementSession {
    /// Original prediction report
    pub original_report: PredictionReport,
    /// Refinement rounds
    pub rounds: Vec<RefinementRound>,
    /// Current best prediction
    pub current_best: PredictionReport,
    /// Has the user marked this as final?
    pub finalized: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. The MiroFish Engine — Ties Everything Together
// ─────────────────────────────────────────────────────────────────────────────

/// Full MiroFish trajectory analysis result
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrajectoryAnalysis {
    /// The scenario template used (if any)
    pub template_id: Option<String>,
    /// Domain
    pub domain: String,
    /// Variables provided
    pub variables: HashMap<String, String>,
    /// The probability flow network
    pub flow_network: ProbabilityFlowNetwork,
    /// Projection curve over time
    pub projection: Vec<ProjectionPoint>,
    /// Action trajectory (step-by-step plan)
    pub trajectory: Trajectory,
    /// Scenario branches from LLM
    pub scenario_report: PredictionReport,
    /// Expected impact score
    pub expected_impact: f64,
    /// Most likely outcome
    pub most_likely_outcome: String,
    /// Calibration stats (if available)
    pub calibration: Option<CalibrationStats>,
    /// Generated at timestamp
    pub generated_at: u64,
}

/// The MiroFish Trajectory Engine
pub struct MiroFishEngine {
    /// LLM client for trajectory generation
    llm: OllamaClient,
    /// Historical predictions for calibration
    history: Vec<PredictionRecord>,
}

impl MiroFishEngine {
    /// Create engine from an existing OllamaClient
    pub fn new(llm: OllamaClient) -> Self {
        Self {
            llm,
            history: Vec::new(),
        }
    }

    /// Run a full trajectory analysis using a template
    pub async fn analyze_with_template(
        &self,
        template: &ScenarioTemplate,
        variables: &HashMap<String, String>,
        beliefs: &[crate::hyper_stigmergy::Belief],
    ) -> Result<TrajectoryAnalysis> {
        // 1. Build probability flow network from template
        let mut network = ProbabilityFlowNetwork::new(
            template.default_states.clone(),
            template.default_transitions.clone(),
            &template.default_states[0].id,
        );

        // 2. Simulate probability flow
        network.simulate(template.suggested_time_steps);
        let projection = network.projection_curve();

        // 3. Generate action trajectory via LLM
        let trajectory = self
            .generate_trajectory(template, variables, beliefs)
            .await?;

        // 4. Run LLM scenario branches
        let scenario_report = self
            .generate_scenario_report(template, variables, beliefs)
            .await?;

        // 5. Compute expected impact
        let expected_impact = network.expected_impact();
        let most_likely = network
            .most_likely_outcome()
            .map(|s| s.description.clone())
            .unwrap_or_else(|| "uncertain".to_string());

        // 6. Calibration
        let calibration = if self.history.len() >= 5 {
            Some(compute_calibration(&self.history))
        } else {
            None
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(TrajectoryAnalysis {
            template_id: Some(template.id.clone()),
            domain: template.domain.to_string(),
            variables: variables.clone(),
            flow_network: network,
            projection,
            trajectory,
            scenario_report,
            expected_impact,
            most_likely_outcome: most_likely,
            calibration,
            generated_at: now,
        })
    }

    /// Generate step-by-step trajectory via LLM
    async fn generate_trajectory(
        &self,
        template: &ScenarioTemplate,
        variables: &HashMap<String, String>,
        beliefs: &[crate::hyper_stigmergy::Belief],
    ) -> Result<Trajectory> {
        let var_text: String = variables
            .iter()
            .map(|(k, v)| format!("- {}: {}", k, v))
            .collect::<Vec<_>>()
            .join("\n");

        let belief_text: String = beliefs
            .iter()
            .take(5)
            .map(|b| format!("- [{:.0}%] {}", b.confidence * 100.0, b.content))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "You are a strategic planning engine. Create a step-by-step action plan.\n\n\
             Scenario: {}\n\
             Domain: {}\n\
             Variables:\n{}\n\
             Business Context:\n{}\n\n\
             Generate 4-6 concrete action steps. For each step provide:\n\
             STEP [N]: [action description]\n\
             OUTCOME: [expected outcome]\n\
             TIME: [time horizon]\n\
             PROBABILITY: [0.0-1.0]\n\
             RESOURCES: [what's needed]\n\
             DEPENDS_ON: [step numbers or 'none']\n\
             RISKS: [specific risks]\n\n\
             Be specific and actionable. Use the business context.",
            template.name, template.domain, var_text, belief_text
        );

        let result = self.llm.generate(&prompt).await;

        let steps = if result.timed_out || result.text.is_empty() {
            // Fallback: generate basic steps
            vec![TrajectoryStep {
                step: 1,
                action: format!("Execute {} strategy", template.domain),
                expected_outcome: "Initial progress toward goal".into(),
                time_horizon: "Month 1-2".into(),
                success_probability: 0.6,
                resources: vec!["Team time".into(), "Budget allocation".into()],
                depends_on: vec![],
                risks: vec!["Execution risk".into()],
            }]
        } else {
            Self::parse_trajectory_steps(&result.text)
        };

        let cumulative_prob = steps.iter().map(|s| s.success_probability).product::<f64>();
        let critical_path: Vec<usize> = steps.iter().map(|s| s.step).collect();

        Ok(Trajectory {
            name: template.name.clone(),
            initial_state: "Current state".into(),
            target_outcome: variables
                .get("target_mrr")
                .or(variables.get("target_metric"))
                .cloned()
                .unwrap_or_else(|| "Goal achieved".into()),
            steps,
            cumulative_probability: cumulative_prob,
            estimated_duration: format!("{} time steps", template.suggested_time_steps),
            critical_path,
        })
    }

    /// Parse LLM output into trajectory steps
    fn parse_trajectory_steps(text: &str) -> Vec<TrajectoryStep> {
        let mut steps = Vec::new();
        let mut current_step: Option<TrajectoryStep> = None;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let upper = trimmed.to_uppercase();

            if upper.starts_with("STEP") {
                // Save previous step if exists
                if let Some(step) = current_step.take() {
                    steps.push(step);
                }
                let step_num = steps.len() + 1;
                let action = trimmed
                    .splitn(2, ':')
                    .nth(1)
                    .unwrap_or(trimmed)
                    .trim()
                    .to_string();
                current_step = Some(TrajectoryStep {
                    step: step_num,
                    action,
                    expected_outcome: String::new(),
                    time_horizon: String::new(),
                    success_probability: 0.6,
                    resources: Vec::new(),
                    depends_on: Vec::new(),
                    risks: Vec::new(),
                });
            } else if let Some(ref mut step) = current_step {
                if upper.starts_with("OUTCOME:") {
                    step.expected_outcome = trimmed.splitn(2, ':').nth(1).unwrap_or("").trim().to_string();
                } else if upper.starts_with("TIME:") {
                    step.time_horizon = trimmed.splitn(2, ':').nth(1).unwrap_or("").trim().to_string();
                } else if upper.starts_with("PROBABILITY:") {
                    let val_str = trimmed.splitn(2, ':').nth(1).unwrap_or("0.6").trim();
                    step.success_probability = val_str
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.')
                        .collect::<String>()
                        .parse::<f64>()
                        .unwrap_or(0.6)
                        .clamp(0.0, 1.0);
                } else if upper.starts_with("RESOURCES:") {
                    let res = trimmed.splitn(2, ':').nth(1).unwrap_or("").trim();
                    step.resources = res.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                } else if upper.starts_with("DEPENDS_ON:") || upper.starts_with("DEPENDS ON:") {
                    let deps = trimmed.splitn(2, ':').nth(1).unwrap_or("").trim();
                    if deps.to_lowercase() != "none" {
                        step.depends_on = deps
                            .split(',')
                            .filter_map(|s| {
                                s.trim()
                                    .chars()
                                    .filter(|c| c.is_ascii_digit())
                                    .collect::<String>()
                                    .parse::<usize>()
                                    .ok()
                            })
                            .collect();
                    }
                } else if upper.starts_with("RISKS:") || upper.starts_with("RISK:") {
                    let risk = trimmed.splitn(2, ':').nth(1).unwrap_or("").trim();
                    step.risks = risk.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                }
            }
        }

        // Push last step
        if let Some(step) = current_step {
            steps.push(step);
        }

        steps
    }

    /// Generate LLM-based scenario report using template
    async fn generate_scenario_report(
        &self,
        template: &ScenarioTemplate,
        variables: &HashMap<String, String>,
        beliefs: &[crate::hyper_stigmergy::Belief],
    ) -> Result<PredictionReport> {
        let var_text: String = variables
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect::<Vec<_>>()
            .join(", ");

        let seeds: Vec<String> = beliefs
            .iter()
            .take(5)
            .map(|b| b.content.clone())
            .collect();

        // Use the base scenario simulator for LLM-based branches
        let config = ScenarioSimulatorConfig {
            num_branches: template.suggested_variants.len().min(4),
            ..ScenarioSimulatorConfig::default()
        };

        let simulator = crate::scenario_simulator::ScenarioSimulator::new(config);
        let report = simulator
            .simulate(
                &format!("{}: {}", template.name, var_text),
                &seeds,
                {
                    let variant_str = template.suggested_variants.join(", ");
                    let variants = vec![variant_str];
                    Some(variants)
                }.as_deref(),
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(report)
    }

    /// Add a historical prediction for back-testing
    pub fn record_prediction(&mut self, record: PredictionRecord) {
        self.history.push(record);
    }

    /// Record the actual outcome of a past prediction
    pub fn record_outcome(&mut self, prediction_id: &str, actual_outcome: &str, was_correct: bool) {
        if let Some(record) = self.history.iter_mut().find(|r| r.id == prediction_id) {
            record.actual_outcome = Some(actual_outcome.to_string());
            record.was_correct = Some(was_correct);
            record.calibration_error = Some((record.predicted_confidence - if was_correct { 1.0 } else { 0.0 }).abs());
        }
    }

    /// Get calibration stats
    pub fn calibration_stats(&self) -> CalibrationStats {
        compute_calibration(&self.history)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probability_flow_basic() {
        let states = vec![
            FlowState { id: "A".into(), description: "Start".into(), probability: 1.0, terminal: false, impact_score: 0.0 },
            FlowState { id: "B".into(), description: "Success".into(), probability: 0.0, terminal: true, impact_score: 10.0 },
            FlowState { id: "C".into(), description: "Failure".into(), probability: 0.0, terminal: true, impact_score: -5.0 },
        ];
        let transitions = vec![
            FlowTransition { from: "A".into(), to: "B".into(), probability: 0.7, trigger: "good".into(), time_estimate: "1m".into() },
            FlowTransition { from: "A".into(), to: "C".into(), probability: 0.3, trigger: "bad".into(), time_estimate: "1m".into() },
        ];

        let mut network = ProbabilityFlowNetwork::new(states, transitions, "A");

        // Initial state: all probability in A
        assert!((network.states[0].probability - 1.0).abs() < 1e-10);
        assert!((network.states[1].probability - 0.0).abs() < 1e-10);

        // After one step: probability flows to B and C
        network.step();
        assert!((network.states[1].probability - 0.7).abs() < 0.01, "B should have ~0.7, got {}", network.states[1].probability);
        assert!((network.states[2].probability - 0.3).abs() < 0.01, "C should have ~0.3, got {}", network.states[2].probability);
    }

    #[test]
    fn test_probability_flow_multi_step() {
        let states = vec![
            FlowState { id: "start".into(), description: "Start".into(), probability: 1.0, terminal: false, impact_score: 0.0 },
            FlowState { id: "mid".into(), description: "Middle".into(), probability: 0.0, terminal: false, impact_score: 2.0 },
            FlowState { id: "end".into(), description: "End".into(), probability: 0.0, terminal: true, impact_score: 10.0 },
        ];
        let transitions = vec![
            FlowTransition { from: "start".into(), to: "mid".into(), probability: 0.6, trigger: "advance".into(), time_estimate: "1m".into() },
            FlowTransition { from: "mid".into(), to: "end".into(), probability: 0.8, trigger: "complete".into(), time_estimate: "1m".into() },
        ];

        let mut network = ProbabilityFlowNetwork::new(states, transitions, "start");
        network.simulate(5);

        // After enough steps, most probability should be in terminal "end"
        let end_prob = network.states.iter().find(|s| s.id == "end").unwrap().probability;
        assert!(end_prob > 0.3, "End state should accumulate probability, got {}", end_prob);

        // Verify probability conservation (total should be ~1.0)
        let total: f64 = network.states.iter().map(|s| s.probability).sum();
        assert!((total - 1.0).abs() < 0.01, "Total probability should be ~1.0, got {}", total);
    }

    #[test]
    fn test_expected_impact() {
        let states = vec![
            FlowState { id: "A".into(), description: "Start".into(), probability: 0.0, terminal: false, impact_score: 0.0 },
            FlowState { id: "good".into(), description: "Good".into(), probability: 0.7, terminal: true, impact_score: 10.0 },
            FlowState { id: "bad".into(), description: "Bad".into(), probability: 0.3, terminal: true, impact_score: -5.0 },
        ];

        let network = ProbabilityFlowNetwork::new(states, vec![], "A");
        let impact = network.expected_impact();
        let expected = (0.7 * 10.0 + 0.3 * -5.0) / (0.7 + 0.3);
        assert!((impact - expected).abs() < 0.01, "Expected impact {}, got {}", expected, impact);
    }

    #[test]
    fn test_projection_curve() {
        let states = vec![
            FlowState { id: "A".into(), description: "Start".into(), probability: 1.0, terminal: false, impact_score: 0.0 },
            FlowState { id: "B".into(), description: "End".into(), probability: 0.0, terminal: true, impact_score: 5.0 },
        ];
        let transitions = vec![
            FlowTransition { from: "A".into(), to: "B".into(), probability: 0.5, trigger: "go".into(), time_estimate: "1m".into() },
        ];

        let mut network = ProbabilityFlowNetwork::new(states, transitions, "A");
        network.simulate(3);

        let curve = network.projection_curve();
        assert_eq!(curve.len(), 4); // initial + 3 steps

        // First point should show all probability in A
        assert!((curve[0].state_probabilities["A"] - 1.0).abs() < 1e-10);

        // B's probability should increase over time
        assert!(curve[3].state_probabilities.get("B").copied().unwrap_or(0.0) > curve[1].state_probabilities.get("B").copied().unwrap_or(0.0));
    }

    #[test]
    fn test_builtin_templates() {
        let templates = builtin_templates();
        assert_eq!(templates.len(), 6);

        // Verify pricing template
        let pricing = templates.iter().find(|t| t.id == "pricing_strategy").unwrap();
        assert_eq!(pricing.domain, ScenarioDomain::PricingStrategy);
        assert!(!pricing.required_variables.is_empty());
        assert!(!pricing.default_states.is_empty());
        assert!(!pricing.default_transitions.is_empty());

        // Verify all templates have required components
        for template in &templates {
            assert!(!template.name.is_empty(), "Template {} has no name", template.id);
            assert!(!template.default_states.is_empty(), "Template {} has no states", template.id);
            assert!(!template.default_transitions.is_empty(), "Template {} has no transitions", template.id);
            assert!(template.suggested_time_steps > 0, "Template {} has 0 time steps", template.id);

            // Verify transitions reference valid states
            let state_ids: Vec<&str> = template.default_states.iter().map(|s| s.id.as_str()).collect();
            for transition in &template.default_transitions {
                assert!(state_ids.contains(&transition.from.as_str()), "Template {} has transition from unknown state '{}'", template.id, transition.from);
                assert!(state_ids.contains(&transition.to.as_str()), "Template {} has transition to unknown state '{}'", template.id, transition.to);
            }
        }
    }

    #[test]
    fn test_calibration_computation() {
        let records = vec![
            PredictionRecord { id: "1".into(), predicted_at: 0, topic: "test".into(), predicted_outcome: "A".into(), predicted_confidence: 0.8, actual_outcome: Some("A".into()), was_correct: Some(true), calibration_error: Some(0.2) },
            PredictionRecord { id: "2".into(), predicted_at: 0, topic: "test".into(), predicted_outcome: "B".into(), predicted_confidence: 0.9, actual_outcome: Some("C".into()), was_correct: Some(false), calibration_error: Some(0.9) },
            PredictionRecord { id: "3".into(), predicted_at: 0, topic: "test".into(), predicted_outcome: "D".into(), predicted_confidence: 0.7, actual_outcome: Some("D".into()), was_correct: Some(true), calibration_error: Some(0.3) },
            PredictionRecord { id: "4".into(), predicted_at: 0, topic: "test".into(), predicted_outcome: "E".into(), predicted_confidence: 0.6, actual_outcome: Some("F".into()), was_correct: Some(false), calibration_error: Some(0.6) },
            PredictionRecord { id: "5".into(), predicted_at: 0, topic: "test".into(), predicted_outcome: "G".into(), predicted_confidence: 0.8, actual_outcome: Some("G".into()), was_correct: Some(true), calibration_error: Some(0.2) },
        ];

        let stats = compute_calibration(&records);
        assert_eq!(stats.total_evaluated, 5);
        assert_eq!(stats.correct, 3);
        assert!((stats.actual_accuracy - 0.6).abs() < 0.01);
        assert!((stats.avg_predicted_confidence - 0.76).abs() < 0.01);
        assert_eq!(stats.direction, "overconfident");
    }

    #[test]
    fn test_calibration_empty() {
        let stats = compute_calibration(&[]);
        assert_eq!(stats.total_evaluated, 0);
        assert_eq!(stats.direction, "insufficient data");
        assert!((stats.adjustment_factor - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_recalibrate_confidence() {
        let stats = CalibrationStats {
            total_evaluated: 10,
            correct: 6,
            avg_predicted_confidence: 0.8,
            actual_accuracy: 0.6,
            calibration_error: 0.2,
            direction: "overconfident".into(),
            adjustment_factor: 0.75, // 0.6 / 0.8
        };

        // 0.8 * 0.75 = 0.6 — recalibrated to match actual accuracy
        let recalibrated = recalibrate_confidence(0.8, &stats);
        assert!((recalibrated - 0.6).abs() < 0.01);

        // Clamping: very low confidence shouldn't go below 0.01
        let low = recalibrate_confidence(0.01, &stats);
        assert!(low >= 0.01);
    }

    #[test]
    fn test_recalibrate_insufficient_data() {
        let stats = CalibrationStats {
            total_evaluated: 3, // < 5 threshold
            correct: 2,
            avg_predicted_confidence: 0.7,
            actual_accuracy: 0.67,
            calibration_error: 0.03,
            direction: "well-calibrated".into(),
            adjustment_factor: 0.95,
        };

        // Should return raw confidence (not enough data)
        let result = recalibrate_confidence(0.8, &stats);
        assert!((result - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_trajectory_steps() {
        let text = "\
STEP 1: Research market pricing benchmarks
OUTCOME: Understand competitor pricing landscape
TIME: Week 1-2
PROBABILITY: 0.9
RESOURCES: Analyst time, market research tools
DEPENDS_ON: none
RISKS: Incomplete data, biased sources

STEP 2: Survey existing customers on pricing sensitivity
OUTCOME: Data on willingness to pay
TIME: Week 2-3
PROBABILITY: 0.75
RESOURCES: Survey tool, customer success team
DEPENDS_ON: 1
RISKS: Low response rate";

        let steps = MiroFishEngine::parse_trajectory_steps(text);
        assert_eq!(steps.len(), 2);

        assert_eq!(steps[0].step, 1);
        assert!(steps[0].action.contains("Research"));
        assert!((steps[0].success_probability - 0.9).abs() < 0.01);
        assert!(steps[0].depends_on.is_empty());

        assert_eq!(steps[1].step, 2);
        assert!(steps[1].action.contains("Survey"));
        assert!((steps[1].success_probability - 0.75).abs() < 0.01);
        assert_eq!(steps[1].depends_on, vec![1]);
    }

    #[test]
    fn test_scenario_domain_display() {
        assert_eq!(ScenarioDomain::PricingStrategy.to_string(), "Pricing Strategy");
        assert_eq!(ScenarioDomain::MarketEntry.to_string(), "Market Entry");
        assert_eq!(ScenarioDomain::Custom("My Domain".into()).to_string(), "My Domain");
    }

    #[test]
    fn test_probability_conservation() {
        // Test that probability is conserved across many steps
        let states = vec![
            FlowState { id: "s1".into(), description: "".into(), probability: 1.0, terminal: false, impact_score: 0.0 },
            FlowState { id: "s2".into(), description: "".into(), probability: 0.0, terminal: false, impact_score: 0.0 },
            FlowState { id: "s3".into(), description: "".into(), probability: 0.0, terminal: true, impact_score: 5.0 },
            FlowState { id: "s4".into(), description: "".into(), probability: 0.0, terminal: true, impact_score: -3.0 },
        ];
        let transitions = vec![
            FlowTransition { from: "s1".into(), to: "s2".into(), probability: 0.5, trigger: "".into(), time_estimate: "".into() },
            FlowTransition { from: "s1".into(), to: "s3".into(), probability: 0.2, trigger: "".into(), time_estimate: "".into() },
            FlowTransition { from: "s2".into(), to: "s3".into(), probability: 0.4, trigger: "".into(), time_estimate: "".into() },
            FlowTransition { from: "s2".into(), to: "s4".into(), probability: 0.3, trigger: "".into(), time_estimate: "".into() },
        ];

        let mut network = ProbabilityFlowNetwork::new(states, transitions, "s1");
        network.simulate(20);

        let total: f64 = network.states.iter().map(|s| s.probability).sum();
        assert!((total - 1.0).abs() < 0.01, "Total probability should be ~1.0 after 20 steps, got {}", total);
    }

    #[test]
    fn test_most_likely_outcome() {
        let states = vec![
            FlowState { id: "start".into(), description: "".into(), probability: 0.0, terminal: false, impact_score: 0.0 },
            FlowState { id: "win".into(), description: "Win".into(), probability: 0.6, terminal: true, impact_score: 10.0 },
            FlowState { id: "lose".into(), description: "Lose".into(), probability: 0.4, terminal: true, impact_score: -5.0 },
        ];

        let network = ProbabilityFlowNetwork::new(states, vec![], "start");
        let outcome = network.most_likely_outcome().unwrap();
        assert_eq!(outcome.id, "win");
    }

    #[test]
    fn test_flow_network_serde() {
        let states = vec![
            FlowState { id: "A".into(), description: "Start".into(), probability: 1.0, terminal: false, impact_score: 0.0 },
        ];
        let network = ProbabilityFlowNetwork::new(states, vec![], "A");

        let json = serde_json::to_string(&network).unwrap();
        let restored: ProbabilityFlowNetwork = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.states.len(), 1);
        assert_eq!(restored.current_state, "A");
    }

    #[test]
    fn test_trajectory_analysis_serde() {
        let analysis = TrajectoryAnalysis {
            template_id: Some("test".into()),
            domain: "Testing".into(),
            variables: HashMap::new(),
            flow_network: ProbabilityFlowNetwork::new(vec![], vec![], "start"),
            projection: vec![],
            trajectory: Trajectory {
                name: "test".into(),
                initial_state: "now".into(),
                target_outcome: "goal".into(),
                steps: vec![],
                cumulative_probability: 0.5,
                estimated_duration: "6 months".into(),
                critical_path: vec![],
            },
            scenario_report: PredictionReport {
                topic: "test".into(),
                seeds: vec![],
                variables: vec![],
                branches: vec![],
                synthesis: "test".into(),
                overall_confidence: 0.5,
                generated_at: 0,
            },
            expected_impact: 5.0,
            most_likely_outcome: "success".into(),
            calibration: None,
            generated_at: 0,
        };

        let json = serde_json::to_string(&analysis).unwrap();
        let restored: TrajectoryAnalysis = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.domain, "Testing");
    }
}
