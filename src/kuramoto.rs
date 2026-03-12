// ═══════════════════════════════════════════════════════════════════════
// Kuramoto Synchronization Module for Hyper-Stigmergic Morphogenesis
// ═══════════════════════════════════════════════════════════════════════
//
// Maps the Kuramoto coupled-oscillator model onto the HSM-II agent
// system, treating each agent as a phase oscillator whose natural
// frequency is derived from its energy score (JW × drives).
//
// HyperEdges provide coupling channels: the coupling strength K_ij
// between agents i and j is the edge weight connecting them.
//
// This engine uses a graph Kuramoto phase evolution law:
//   dθ_i/dt = ω_i + (K/N) Σ_j A_ij · w_ij · sin(θ_j − θ_i)
//                  + δ · dispersion_term
//
// Note: this is not a full generalized Kuramoto-Sivashinsky PDE solver.
// The δ term below is a network-level dispersion/diffusion correction.
//
// The order parameter R ∈ [0,1] measures global synchronization:
//   R · e^{iψ} = (1/N) Σ_j e^{iθ_j}
//
// Important: R is a coherence metric, not a stand-alone chaos detector.
//
// Council acts as a synergetic field generator: it broadcasts a
// reference phase that biases all oscillators toward coherence.

use std::collections::{HashMap, HashSet, VecDeque};
use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::agent::AgentId;

// ── Configuration ─────────────────────────────────────────────────────

/// Configuration for the Kuramoto synchronization dynamics.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KuramotoConfig {
    /// Global coupling strength K. Higher → faster synchronization.
    pub coupling_strength: f64,
    /// Integration time step dt.
    pub dt: f64,
    /// Graph dispersion/diffusion coefficient δ.
    /// δ > 0 adds a neighborhood Laplacian-like correction; δ = 0 is pure Kuramoto.
    pub dispersion: f64,
    /// Council reference phase influence strength (0..1).
    /// When the council fires, it broadcasts a reference phase;
    /// this weight determines how strongly agents pull toward it.
    pub council_influence: f64,
    /// Noise amplitude for stochastic perturbation (Langevin term).
    pub noise_amplitude: f64,
    /// Minimum coupling weight below which edges are ignored.
    pub min_edge_weight: f64,
    /// Enable frustration (anti-phase coupling for contrarian agents).
    pub enable_frustration: bool,
    /// Enable phase-field correction terms inspired by generalized KS structure.
    pub enable_phase_field: bool,
    /// Anti-diffusive growth coefficient on graph Laplacian of phase.
    pub phase_field_growth: f64,
    /// Hyperviscosity damping coefficient on graph bi-Laplacian of phase.
    pub phase_field_hyperviscosity: f64,
    /// Dispersion-like skew coefficient on graph odd derivative proxy.
    pub phase_field_dispersion: f64,
}

impl Default for KuramotoConfig {
    fn default() -> Self {
        Self {
            coupling_strength: 1.0,
            dt: 0.05,
            dispersion: 0.0,
            council_influence: 0.3,
            noise_amplitude: 0.01,
            min_edge_weight: 0.01,
            enable_frustration: false,
            enable_phase_field: false,
            phase_field_growth: 0.0,
            phase_field_hyperviscosity: 0.0,
            phase_field_dispersion: 0.0,
        }
    }
}

// ── Per-agent oscillator state ────────────────────────────────────────

/// Oscillator state for a single agent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Oscillator {
    /// Agent ID this oscillator belongs to.
    pub agent_id: AgentId,
    /// Current phase θ ∈ [0, 2π).
    pub phase: f64,
    /// Natural frequency ω derived from agent energy.
    pub natural_freq: f64,
    /// Phase velocity dθ/dt from last step.
    pub velocity: f64,
    /// Running average of coupling received (diagnostic).
    pub coupling_received: f64,
    /// Frustration sign: +1 for conformist, −1 for contrarian.
    pub frustration: f64,
}

impl Oscillator {
    pub fn new(agent_id: AgentId, initial_phase: f64, natural_freq: f64) -> Self {
        Self {
            agent_id,
            phase: wrap_phase(initial_phase),
            natural_freq,
            velocity: 0.0,
            coupling_received: 0.0,
            frustration: 1.0,
        }
    }
}

// ── Synchronization snapshot (for API / visualization) ────────────────

/// Snapshot of the synchronization state, serializable for the web API.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct KuramotoSnapshot {
    /// Order parameter R ∈ [0, 1]. 1 = perfect sync, 0 = incoherent.
    pub order_parameter: f64,
    /// Mean phase ψ of the collective.
    pub mean_phase: f64,
    /// Per-agent phase and frequency data.
    pub oscillators: Vec<OscillatorSnapshot>,
    /// Current configuration.
    pub config: Option<KuramotoConfig>,
    /// Total synchronization steps executed.
    pub step_count: u64,
    /// Phase coherence histogram (8 bins over [0, 2π)).
    pub phase_histogram: [u32; 8],
    /// Cluster detection: groups of agents within π/6 phase distance.
    pub clusters: Vec<Vec<AgentId>>,
    /// Supplemental diagnostics to interpret coherence dynamics.
    pub diagnostics: KuramotoDiagnostics,
    /// Runtime warnings when model prerequisites may be violated.
    pub preflight_warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OscillatorSnapshot {
    pub agent_id: AgentId,
    pub phase: f64,
    pub natural_freq: f64,
    pub velocity: f64,
    pub coupling_received: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct KuramotoDiagnostics {
    /// Normalized Shannon entropy of phase distribution in [0, 1].
    pub phase_entropy: f64,
    /// Standard deviation of oscillator velocities.
    pub velocity_stddev: f64,
    /// Stddev of recent order-parameter history (volatility proxy).
    pub r_window_stddev: f64,
}

// ── Core engine ───────────────────────────────────────────────────────

/// The Kuramoto synchronization engine.
///
/// Manages oscillator states and performs phase-coupling updates
/// driven by the hypergraph adjacency structure.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KuramotoEngine {
    pub config: KuramotoConfig,
    pub oscillators: HashMap<AgentId, Oscillator>,
    pub step_count: u64,
    /// Cached order parameter from last step.
    pub last_order_parameter: f64,
    /// Cached mean phase from last step.
    pub last_mean_phase: f64,
    /// Council reference phase (set when council fires).
    pub council_ref_phase: Option<f64>,
    /// History of R values for trend analysis (ring buffer, last 256).
    pub r_history: Vec<f64>,
    /// Last runtime warnings about violated assumptions or risky parameters.
    pub last_preflight_warnings: Vec<String>,
    /// Baseline coupling before runtime throttling/gating.
    pub base_coupling_strength: f64,
    /// Baseline council influence before runtime throttling/gating.
    pub base_council_influence: f64,
    /// Baseline noise amplitude before runtime entropy compensation.
    pub base_noise_amplitude: f64,
    /// Adaptive gain scale [0, 1] controlled by quality guardrails.
    pub adaptive_gain_scale: f64,
    /// Previous quality samples for trend checks.
    pub prev_quality_coherence: Option<f64>,
    pub prev_quality_reward: Option<f64>,
    /// Consecutive quality-degradation events.
    pub quality_degrade_streak: u32,
}

impl Default for KuramotoEngine {
    fn default() -> Self {
        Self {
            config: KuramotoConfig::default(),
            oscillators: HashMap::new(),
            step_count: 0,
            last_order_parameter: 0.0,
            last_mean_phase: 0.0,
            council_ref_phase: None,
            r_history: Vec::new(),
            last_preflight_warnings: Vec::new(),
            base_coupling_strength: KuramotoConfig::default().coupling_strength,
            base_council_influence: KuramotoConfig::default().council_influence,
            base_noise_amplitude: KuramotoConfig::default().noise_amplitude,
            adaptive_gain_scale: 1.0,
            prev_quality_coherence: None,
            prev_quality_reward: None,
            quality_degrade_streak: 0,
        }
    }
}

impl KuramotoEngine {
    pub fn new(config: KuramotoConfig) -> Self {
        Self {
            base_coupling_strength: config.coupling_strength,
            base_council_influence: config.council_influence,
            base_noise_amplitude: config.noise_amplitude,
            config,
            ..Default::default()
        }
    }

    // ── Agent lifecycle ───────────────────────────────────────────────

    /// Register an agent as an oscillator. Natural frequency ω is
    /// derived from the agent's JW score and drives:
    ///   ω = base_freq + jw_contribution + drive_contribution
    pub fn register_agent(
        &mut self,
        agent_id: AgentId,
        jw: f64,
        curiosity: f64,
        transcendence: f64,
    ) {
        let natural_freq = compute_natural_frequency(jw, curiosity, transcendence);
        // Deterministic initial phase spread based on agent ID
        let initial_phase = (agent_id as f64 * 2.399_963_229_728_653) % (2.0 * PI);
        let osc = Oscillator::new(agent_id, initial_phase, natural_freq);
        // Frustration (contrarian coupling for Critics/Explorers) is set
        // externally via set_frustration() based on agent Role.
        self.oscillators.insert(agent_id, osc);
    }

    /// Remove an agent's oscillator (agent died or was removed).
    pub fn remove_agent(&mut self, agent_id: AgentId) {
        self.oscillators.remove(&agent_id);
    }

    /// Update natural frequency when agent's energy changes.
    pub fn update_frequency(
        &mut self,
        agent_id: AgentId,
        jw: f64,
        curiosity: f64,
        transcendence: f64,
    ) {
        if let Some(osc) = self.oscillators.get_mut(&agent_id) {
            osc.natural_freq = compute_natural_frequency(jw, curiosity, transcendence);
        }
    }

    /// Set frustration sign for contrarian agents (Critics, Explorers).
    pub fn set_frustration(&mut self, agent_id: AgentId, frustration: f64) {
        if let Some(osc) = self.oscillators.get_mut(&agent_id) {
            osc.frustration = frustration;
        }
    }

    // ── Council integration ───────────────────────────────────────────

    /// Set the council reference phase. Computed from the council's
    /// synthesis confidence and dominant belief direction.
    pub fn set_council_phase(&mut self, phase: f64) {
        self.council_ref_phase = Some(wrap_phase(phase));
    }

    /// Clear the council reference phase (between council sessions).
    pub fn clear_council_phase(&mut self) {
        self.council_ref_phase = None;
    }

    // ── Core step ─────────────────────────────────────────────────────

    /// Perform one Kuramoto integration step.
    ///
    /// `adjacency` maps each agent to its neighbors + edge weights.
    /// This is extracted from the hypergraph each tick.
    ///
    /// Returns the new order parameter R.
    pub fn step(&mut self, adjacency: &HashMap<AgentId, Vec<(AgentId, f64)>>) -> f64 {
        if self.oscillators.is_empty() {
            return 1.0;
        }

        let dt = self.config.dt;
        let k = self.config.coupling_strength;
        let delta = self.config.dispersion;
        let council_k = self.config.council_influence;
        let noise_amp = self.config.noise_amplitude;
        let phase_field_on = self.config.enable_phase_field;
        self.last_preflight_warnings = self.preflight_warnings(adjacency);
        if !self.last_preflight_warnings.is_empty()
            && (self.step_count == 0 || self.step_count % 100 == 0)
        {
            eprintln!(
                "[kuramoto] preflight warnings: {}",
                self.last_preflight_warnings.join(" | ")
            );
        }

        // Snapshot current phases for synchronous update
        let phases: HashMap<AgentId, f64> = self
            .oscillators
            .iter()
            .map(|(&id, osc)| (id, osc.phase))
            .collect();
        let laplacians = if phase_field_on {
            self.compute_phase_laplacian(adjacency, &phases)
        } else {
            HashMap::new()
        };

        // Compute phase derivatives
        let mut derivatives: HashMap<AgentId, f64> = HashMap::new();

        for (&agent_id, osc) in &self.oscillators {
            let theta_i = phases[&agent_id];
            let mut d_theta = osc.natural_freq;

            // Coupling from hypergraph neighbors (requires ≥2 oscillators)
            let mut coupling_sum = 0.0;
            let mut neighbor_count = 0.0;

            if let Some(neighbors) = adjacency.get(&agent_id) {
                for &(neighbor_id, weight) in neighbors {
                    if weight < self.config.min_edge_weight {
                        continue;
                    }
                    if let Some(&theta_j) = phases.get(&neighbor_id) {
                        let phase_diff = theta_j - theta_i;
                        coupling_sum += weight * (osc.frustration * phase_diff).sin();
                        neighbor_count += weight;
                    }
                }
            }

            if neighbor_count > 0.0 {
                d_theta += (k / neighbor_count) * coupling_sum;
            }

            // Council reference phase coupling (synchronizing bias).
            // Antipodal offsets (≈π) are unstable fixed points when noise is zero.
            if let Some(ref_phase) = self.council_ref_phase {
                d_theta += council_k * (ref_phase - theta_i).sin();
            }

            // Graph dispersion term: neighborhood Laplacian-like correction.
            // This is a heuristic network diffusion term, not a PDE u_xxx/u_xxxx discretization.
            if delta.abs() > 1e-12 && neighbor_count > 0.0 {
                let mut laplacian = 0.0;
                if let Some(neighbors) = adjacency.get(&agent_id) {
                    for &(neighbor_id, weight) in neighbors {
                        if let Some(&theta_j) = phases.get(&neighbor_id) {
                            laplacian += weight * signed_phase_delta(theta_i, theta_j);
                        }
                    }
                    if !neighbors.is_empty() {
                        laplacian /= neighbor_count;
                    }
                }
                d_theta += delta * laplacian;
            }

            if phase_field_on && neighbor_count > 0.0 {
                let lap_i = *laplacians.get(&agent_id).unwrap_or(&0.0);
                let mut bi_lap = 0.0;
                let mut skew = 0.0;

                if let Some(neighbors) = adjacency.get(&agent_id) {
                    for &(neighbor_id, weight) in neighbors {
                        if weight < self.config.min_edge_weight {
                            continue;
                        }
                        let lap_j = *laplacians.get(&neighbor_id).unwrap_or(&0.0);
                        bi_lap += weight * (lap_j - lap_i);
                        if let Some(&theta_j) = phases.get(&neighbor_id) {
                            let orient = signed_phase_delta(theta_i, theta_j).signum();
                            skew += weight * orient * (lap_j - lap_i);
                        }
                    }
                    bi_lap /= neighbor_count;
                    skew /= neighbor_count;
                }

                d_theta += self.config.phase_field_growth * lap_i;
                d_theta -= self.config.phase_field_hyperviscosity * bi_lap;
                d_theta += self.config.phase_field_dispersion * skew;
            }

            // Stochastic noise (Langevin term)
            if noise_amp > 0.0 {
                // Simple deterministic "noise" based on step count + agent_id
                // (avoiding rand dependency; real noise would use thread_rng)
                let pseudo_noise =
                    ((self.step_count as f64 * 0.618 + agent_id as f64 * 1.324) % 1.0 - 0.5) * 2.0;
                d_theta += noise_amp * pseudo_noise;
            }

            derivatives.insert(agent_id, d_theta);
        }

        // Euler integration + update
        for (agent_id, osc) in self.oscillators.iter_mut() {
            if let Some(&d_theta) = derivatives.get(agent_id) {
                osc.velocity = d_theta;
                osc.phase = wrap_phase(osc.phase + d_theta * dt);
                osc.coupling_received = d_theta - osc.natural_freq;
            }
        }

        self.step_count += 1;

        // Compute order parameter
        let (r, psi) = self.compute_order_parameter();
        self.last_order_parameter = r;
        self.last_mean_phase = psi;

        // Ring buffer for R history
        self.r_history.push(r);
        if self.r_history.len() > 256 {
            self.r_history.remove(0);
        }

        r
    }

    // ── Observables ───────────────────────────────────────────────────

    /// Compute the Kuramoto order parameter.
    /// R · e^{iψ} = (1/N) Σ_j e^{iθ_j}
    /// Returns (R, ψ). R tracks coherence only.
    pub fn compute_order_parameter(&self) -> (f64, f64) {
        let n = self.oscillators.len();
        if n == 0 {
            return (1.0, 0.0);
        }

        let mut sum_cos = 0.0;
        let mut sum_sin = 0.0;
        for osc in self.oscillators.values() {
            sum_cos += osc.phase.cos();
            sum_sin += osc.phase.sin();
        }
        sum_cos /= n as f64;
        sum_sin /= n as f64;

        let r = (sum_cos * sum_cos + sum_sin * sum_sin).sqrt();
        let psi = sum_sin.atan2(sum_cos);

        (r, wrap_phase(psi))
    }

    /// Detect phase clusters: groups of agents within `threshold` radians.
    pub fn detect_clusters(&self, threshold: f64) -> Vec<Vec<AgentId>> {
        let mut agents: Vec<(AgentId, f64)> = self
            .oscillators
            .iter()
            .map(|(&id, osc)| (id, osc.phase))
            .collect();
        agents.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut clusters: Vec<Vec<AgentId>> = Vec::new();
        let mut current_cluster: Vec<AgentId> = Vec::new();

        for &(agent_id, phase) in agents.iter() {
            if current_cluster.is_empty() {
                current_cluster.push(agent_id);
            } else {
                let first_phase = agents
                    .iter()
                    .find(|(id, _)| *id == current_cluster[0])
                    .map(|(_, p)| *p)
                    .unwrap_or(0.0);
                let d = phase_distance(phase, first_phase);
                if d < threshold {
                    current_cluster.push(agent_id);
                } else {
                    if current_cluster.len() > 1 {
                        clusters.push(current_cluster.clone());
                    }
                    current_cluster = vec![agent_id];
                }
            }
        }
        if current_cluster.len() > 1 {
            clusters.push(current_cluster);
        }

        // Check wrap-around: if first and last clusters are close, merge
        if clusters.len() >= 2 {
            let first_phase = self
                .oscillators
                .get(&clusters[0][0])
                .map(|o| o.phase)
                .unwrap_or(0.0);
            let last_phase = self
                .oscillators
                .get(clusters.last().unwrap().last().unwrap())
                .map(|o| o.phase)
                .unwrap_or(0.0);
            if phase_distance(first_phase, last_phase) < threshold {
                let last = clusters.pop().unwrap();
                clusters[0] = [last, clusters[0].clone()].concat();
            }
        }

        clusters
    }

    /// Build a phase histogram with `bins` equal-width bins over [0, 2π).
    pub fn phase_histogram(&self) -> [u32; 8] {
        let mut hist = [0u32; 8];
        let bin_width = 2.0 * PI / 8.0;
        for osc in self.oscillators.values() {
            let bin = ((osc.phase / bin_width) as usize).min(7);
            hist[bin] += 1;
        }
        hist
    }

    /// Get the highest-energy agent (leader oscillator).
    pub fn leader(&self) -> Option<AgentId> {
        self.oscillators
            .values()
            .max_by(|a, b| {
                a.natural_freq
                    .partial_cmp(&b.natural_freq)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|osc| osc.agent_id)
    }

    /// Get synchronization trend: positive = converging, negative = diverging.
    pub fn sync_trend(&self) -> f64 {
        if self.r_history.len() < 10 {
            return 0.0;
        }
        let recent = &self.r_history[self.r_history.len() - 10..];
        let earlier =
            &self.r_history[self.r_history.len().saturating_sub(20)..self.r_history.len() - 10];
        if earlier.is_empty() {
            return 0.0;
        }
        let recent_avg: f64 = recent.iter().sum::<f64>() / recent.len() as f64;
        let earlier_avg: f64 = earlier.iter().sum::<f64>() / earlier.len() as f64;
        recent_avg - earlier_avg
    }

    /// Supplemental diagnostics beyond R.
    /// These are observational proxies and do not by themselves prove chaos.
    pub fn diagnostics(&self) -> KuramotoDiagnostics {
        let phase_entropy = normalized_phase_entropy(&self.phase_histogram());
        let velocities: Vec<f64> = self.oscillators.values().map(|o| o.velocity).collect();
        let velocity_stddev = stddev(&velocities);

        let recent_len = self.r_history.len().min(32);
        let r_window_stddev = if recent_len > 1 {
            stddev(&self.r_history[self.r_history.len() - recent_len..])
        } else {
            0.0
        };

        KuramotoDiagnostics {
            phase_entropy,
            velocity_stddev,
            r_window_stddev,
        }
    }

    /// Checks common prerequisites for stable, interpretable runs.
    pub fn preflight_warnings(
        &self,
        adjacency: &HashMap<AgentId, Vec<(AgentId, f64)>>,
    ) -> Vec<String> {
        let mut warnings = Vec::new();
        let n = self.oscillators.len();

        if n < 2 {
            warnings.push(
                "fewer than 2 oscillators: peer coupling is inactive; only natural/council terms apply"
                    .to_string(),
            );
        }

        if self.config.dt <= 0.0 {
            warnings.push("dt must be > 0 for valid integration".to_string());
        }
        if self.config.enable_phase_field
            && self.config.phase_field_growth > self.config.phase_field_hyperviscosity
            && self.config.phase_field_hyperviscosity > 0.0
        {
            warnings.push(
                "phase-field growth exceeds hyperviscosity damping; unstable pattern growth likely"
                    .to_string(),
            );
        }

        let max_degree = self
            .oscillators
            .keys()
            .map(|id| {
                adjacency
                    .get(id)
                    .map(|nbrs| {
                        nbrs.iter()
                            .filter(|(_, w)| *w >= self.config.min_edge_weight)
                            .map(|(_, w)| *w)
                            .sum::<f64>()
                    })
                    .unwrap_or(0.0)
            })
            .fold(0.0, f64::max);
        let stiffness = self.config.dt
            * (self.config.coupling_strength.abs()
                + self.config.council_influence.abs()
                + self.config.dispersion.abs())
            * max_degree.max(1.0);
        if stiffness > 1.0 {
            warnings.push(format!(
                "dt may be too large for current coupling (heuristic stiffness={:.3} > 1.0)",
                stiffness
            ));
        }

        if n >= 2 && !adjacency.is_empty() {
            let lcc_ratio = self.largest_component_ratio(adjacency);
            if lcc_ratio < 0.8 {
                warnings.push(format!(
                    "graph weakly connected (largest component ratio {:.2} < 0.80)",
                    lcc_ratio
                ));
            }
        }

        if let Some(ref_phase) = self.council_ref_phase {
            if self.config.noise_amplitude == 0.0
                && self
                    .oscillators
                    .values()
                    .any(|o| (phase_distance(o.phase, ref_phase) - PI).abs() < 1e-6)
            {
                warnings.push(
                    "one or more oscillators are exactly antipodal to council phase with zero noise; convergence may stall"
                        .to_string(),
                );
            }
        }

        warnings
    }

    fn largest_component_ratio(&self, adjacency: &HashMap<AgentId, Vec<(AgentId, f64)>>) -> f64 {
        let nodes: HashSet<AgentId> = self.oscillators.keys().copied().collect();
        if nodes.is_empty() {
            return 1.0;
        }

        let mut visited: HashSet<AgentId> = HashSet::new();
        let mut largest = 0usize;

        for &start in &nodes {
            if visited.contains(&start) {
                continue;
            }
            let mut q = VecDeque::new();
            q.push_back(start);
            visited.insert(start);
            let mut size = 0usize;

            while let Some(cur) = q.pop_front() {
                size += 1;
                if let Some(neighbors) = adjacency.get(&cur) {
                    for &(next, weight) in neighbors {
                        if weight < self.config.min_edge_weight || !nodes.contains(&next) {
                            continue;
                        }
                        if visited.insert(next) {
                            q.push_back(next);
                        }
                    }
                }
            }
            largest = largest.max(size);
        }

        largest as f64 / nodes.len() as f64
    }

    fn compute_phase_laplacian(
        &self,
        adjacency: &HashMap<AgentId, Vec<(AgentId, f64)>>,
        phases: &HashMap<AgentId, f64>,
    ) -> HashMap<AgentId, f64> {
        let mut out = HashMap::new();
        for (&agent_id, &theta_i) in phases {
            let mut sum = 0.0;
            let mut w_sum = 0.0;
            if let Some(neighbors) = adjacency.get(&agent_id) {
                for &(neighbor_id, weight) in neighbors {
                    if weight < self.config.min_edge_weight {
                        continue;
                    }
                    if let Some(&theta_j) = phases.get(&neighbor_id) {
                        sum += weight * signed_phase_delta(theta_i, theta_j);
                        w_sum += weight;
                    }
                }
            }
            out.insert(agent_id, if w_sum > 0.0 { sum / w_sum } else { 0.0 });
        }
        out
    }

    // ── Snapshot ──────────────────────────────────────────────────────

    /// Generate a serializable snapshot for the web API / visualization.
    pub fn snapshot(&self) -> KuramotoSnapshot {
        let (r, psi) = self.compute_order_parameter();
        let clusters = self.detect_clusters(PI / 6.0);

        KuramotoSnapshot {
            order_parameter: r,
            mean_phase: psi,
            oscillators: self
                .oscillators
                .values()
                .map(|osc| OscillatorSnapshot {
                    agent_id: osc.agent_id,
                    phase: osc.phase,
                    natural_freq: osc.natural_freq,
                    velocity: osc.velocity,
                    coupling_received: osc.coupling_received,
                })
                .collect(),
            config: Some(self.config.clone()),
            step_count: self.step_count,
            phase_histogram: self.phase_histogram(),
            clusters,
            diagnostics: self.diagnostics(),
            preflight_warnings: self.last_preflight_warnings.clone(),
        }
    }
}

// ── Helper functions ──────────────────────────────────────────────────

/// Compute natural frequency from agent properties.
/// ω = 0.5 + 0.3·JW + 0.1·curiosity + 0.1·transcendence
/// This ensures all agents oscillate, with higher-energy agents faster.
fn compute_natural_frequency(jw: f64, curiosity: f64, transcendence: f64) -> f64 {
    let base = 0.5;
    let jw_contrib = 0.3 * jw.max(0.0).min(10.0);
    let drive_contrib = 0.1 * curiosity.max(0.0).min(1.0) + 0.1 * transcendence.max(0.0).min(1.0);
    base + jw_contrib + drive_contrib
}

/// Wrap phase to [0, 2π).
fn wrap_phase(theta: f64) -> f64 {
    let two_pi = 2.0 * PI;
    let wrapped = theta % two_pi;
    if wrapped < 0.0 {
        wrapped + two_pi
    } else {
        wrapped
    }
}

/// Circular distance between two phases.
fn phase_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).abs() % (2.0 * PI);
    d.min(2.0 * PI - d)
}

fn signed_phase_delta(theta_i: f64, theta_j: f64) -> f64 {
    let mut d = (theta_j - theta_i) % (2.0 * PI);
    if d > PI {
        d -= 2.0 * PI;
    } else if d < -PI {
        d += 2.0 * PI;
    }
    d
}

fn stddev(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let var = values
        .iter()
        .map(|v| {
            let d = v - mean;
            d * d
        })
        .sum::<f64>()
        / values.len() as f64;
    var.sqrt()
}

fn normalized_phase_entropy(hist: &[u32; 8]) -> f64 {
    let total: u32 = hist.iter().sum();
    if total == 0 {
        return 0.0;
    }
    let total_f = total as f64;
    let mut h = 0.0;
    for &count in hist {
        if count == 0 {
            continue;
        }
        let p = count as f64 / total_f;
        h -= p * p.ln();
    }
    let h_max = (hist.len() as f64).ln();
    if h_max <= 0.0 {
        0.0
    } else {
        h / h_max
    }
}

/// Extract Kuramoto adjacency from the HSM-II world state.
///
/// For each agent, collects all other agents it shares a HyperEdge with,
/// along with the edge weight as coupling strength.
pub fn build_adjacency(
    agents: &[crate::agent::Agent],
    edges: &[crate::hyper_stigmergy::HyperEdge],
    _adjacency_map: &HashMap<AgentId, Vec<usize>>,
) -> HashMap<AgentId, Vec<(AgentId, f64)>> {
    let mut adj: HashMap<AgentId, Vec<(AgentId, f64)>> = HashMap::new();

    for agent in agents {
        adj.entry(agent.id).or_default();
    }

    for edge in edges {
        let weight = edge.weight;
        let participants = &edge.participants;
        for (i, &a) in participants.iter().enumerate() {
            for (j, &b) in participants.iter().enumerate() {
                if i != j {
                    adj.entry(a).or_default().push((b, weight));
                }
            }
        }
    }

    // Deduplicate: if multiple edges connect the same pair, sum weights
    for (_, neighbors) in adj.iter_mut() {
        let mut merged: HashMap<AgentId, f64> = HashMap::new();
        for &(neighbor, weight) in neighbors.iter() {
            *merged.entry(neighbor).or_insert(0.0) += weight;
        }
        *neighbors = merged.into_iter().collect();
    }

    adj
}

/// Compute a council reference phase from confidence and synthesis quality.
/// Maps the confidence score [0, 1] to a phase that represents
/// "collective agreement direction".
pub fn confidence_to_phase(confidence: f64, coverage: f64) -> f64 {
    // High confidence → phase near 0 (synchronized target)
    // Low confidence → phase near π (opposition direction)
    // Coverage modulates the spread
    let base_phase = PI * (1.0 - confidence.max(0.0).min(1.0));
    let coverage_mod = 0.2 * (1.0 - coverage.max(0.0).min(1.0));
    wrap_phase(base_phase + coverage_mod)
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_phase() {
        assert!((wrap_phase(0.0) - 0.0).abs() < 1e-10);
        assert!((wrap_phase(2.0 * PI) - 0.0).abs() < 1e-10);
        assert!((wrap_phase(-PI) - PI).abs() < 1e-10);
        assert!((wrap_phase(3.0 * PI) - PI).abs() < 1e-10);
    }

    #[test]
    fn test_phase_distance() {
        assert!((phase_distance(0.0, 0.0)).abs() < 1e-10);
        assert!((phase_distance(0.0, PI) - PI).abs() < 1e-10);
        // Wrap-around: distance between 0.1 and 2π−0.1 should be 0.2
        assert!((phase_distance(0.1, 2.0 * PI - 0.1) - 0.2).abs() < 1e-10);
    }

    #[test]
    fn test_natural_frequency() {
        let f = compute_natural_frequency(1.0, 0.5, 0.5);
        assert!(f > 0.5); // Base frequency
        assert!(f < 2.0); // Bounded
    }

    #[test]
    fn test_order_parameter_synchronized() {
        let mut engine = KuramotoEngine::default();
        // All agents at same phase → R = 1
        for i in 0..5 {
            let osc = Oscillator::new(i, 0.0, 1.0);
            engine.oscillators.insert(i, osc);
        }
        let (r, _) = engine.compute_order_parameter();
        assert!((r - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_order_parameter_desynchronized() {
        let mut engine = KuramotoEngine::default();
        // Agents evenly spread → R ≈ 0
        for i in 0..100 {
            let phase = 2.0 * PI * (i as f64) / 100.0;
            let osc = Oscillator::new(i, phase, 1.0);
            engine.oscillators.insert(i, osc);
        }
        let (r, _) = engine.compute_order_parameter();
        assert!(r < 0.1);
    }

    #[test]
    fn test_step_converges() {
        let mut engine = KuramotoEngine::new(KuramotoConfig {
            coupling_strength: 5.0,
            dt: 0.1,
            ..Default::default()
        });

        // Create 4 agents with similar frequencies, spread phases
        for i in 0..4u64 {
            let phase = PI / 2.0 * i as f64;
            let osc = Oscillator::new(i, phase, 1.0);
            engine.oscillators.insert(i, osc);
        }

        // Fully connected adjacency
        let mut adj: HashMap<AgentId, Vec<(AgentId, f64)>> = HashMap::new();
        for i in 0..4u64 {
            let neighbors: Vec<(AgentId, f64)> =
                (0..4u64).filter(|&j| j != i).map(|j| (j, 1.0)).collect();
            adj.insert(i, neighbors);
        }

        let r_initial = engine.compute_order_parameter().0;

        // Run many steps
        for _ in 0..200 {
            engine.step(&adj);
        }

        let r_final = engine.compute_order_parameter().0;
        // With strong coupling, should converge
        assert!(r_final > r_initial || r_final > 0.9);
    }

    #[test]
    fn test_cluster_detection() {
        let mut engine = KuramotoEngine::default();
        // Two clusters: agents 0,1,2 near phase 0; agents 3,4,5 near phase π
        for i in 0..3u64 {
            let osc = Oscillator::new(i, 0.1 * i as f64, 1.0);
            engine.oscillators.insert(i, osc);
        }
        for i in 3..6u64 {
            let osc = Oscillator::new(i, PI + 0.1 * (i - 3) as f64, 1.0);
            engine.oscillators.insert(i, osc);
        }

        let clusters = engine.detect_clusters(PI / 4.0);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn test_council_phase_influence() {
        let mut engine = KuramotoEngine::new(KuramotoConfig {
            coupling_strength: 0.0, // No peer coupling
            council_influence: 5.0, // Very strong council pull
            dt: 0.05,
            noise_amplitude: 0.0, // No noise
            ..Default::default()
        });

        // Agent at phase π−0.1 with zero natural freq (no spinning)
        // (Exactly at π is an unstable fixed point where sin(0−π) = 0)
        let osc = Oscillator::new(0, PI - 0.1, 0.0);
        engine.oscillators.insert(0, osc);
        engine.set_council_phase(0.0);

        let adj: HashMap<AgentId, Vec<(AgentId, f64)>> = HashMap::new();

        let initial_dist = phase_distance(PI - 0.1, 0.0);

        // Run steps — agent should move toward 0
        for _ in 0..200 {
            engine.step(&adj);
        }

        let final_phase = engine.oscillators[&0].phase;
        let final_dist = phase_distance(final_phase, 0.0);
        // Should be closer to 0 than initially
        assert!(
            final_dist < initial_dist * 0.5,
            "Expected convergence: initial_dist={:.3}, final_dist={:.3}, phase={:.3}",
            initial_dist,
            final_dist,
            final_phase
        );
    }

    #[test]
    fn test_preflight_warns_antipodal_council_no_noise() {
        let mut engine = KuramotoEngine::new(KuramotoConfig {
            coupling_strength: 0.0,
            council_influence: 1.0,
            noise_amplitude: 0.0,
            ..Default::default()
        });
        engine.oscillators.insert(0, Oscillator::new(0, PI, 0.0));
        engine.set_council_phase(0.0);
        let adj: HashMap<AgentId, Vec<(AgentId, f64)>> = HashMap::new();
        let warnings = engine.preflight_warnings(&adj);
        assert!(warnings.iter().any(|w| w.contains("antipodal")));
    }

    #[test]
    fn test_diagnostics_entropy_bounds() {
        let mut engine = KuramotoEngine::default();
        for i in 0..8u64 {
            let phase = (i as f64) * 2.0 * PI / 8.0;
            engine.oscillators.insert(i, Oscillator::new(i, phase, 1.0));
        }
        let d = engine.diagnostics();
        assert!((0.0..=1.0).contains(&d.phase_entropy));
        assert!(d.phase_entropy > 0.9);
    }

    #[test]
    fn test_ab_coupling_improves_coherence_vs_baseline() {
        // Same initial oscillators and topology for both runs.
        let mut with_coupling = KuramotoEngine::new(KuramotoConfig {
            coupling_strength: 3.0,
            council_influence: 0.0,
            noise_amplitude: 0.0,
            dt: 0.05,
            ..Default::default()
        });
        let mut baseline = KuramotoEngine::new(KuramotoConfig {
            coupling_strength: 0.0,
            council_influence: 0.0,
            noise_amplitude: 0.0,
            dt: 0.05,
            ..Default::default()
        });

        for i in 0..12u64 {
            let phase = 2.0 * PI * (i as f64) / 12.0;
            // Slightly heterogeneous frequencies, identical in both runs.
            let natural = 0.9 + (i as f64) * 0.02;
            with_coupling
                .oscillators
                .insert(i, Oscillator::new(i, phase, natural));
            baseline
                .oscillators
                .insert(i, Oscillator::new(i, phase, natural));
        }

        let mut adj: HashMap<AgentId, Vec<(AgentId, f64)>> = HashMap::new();
        for i in 0..12u64 {
            let neighbors: Vec<(AgentId, f64)> =
                (0..12u64).filter(|&j| j != i).map(|j| (j, 1.0)).collect();
            adj.insert(i, neighbors);
        }

        let r0 = with_coupling.compute_order_parameter().0;
        for _ in 0..400 {
            with_coupling.step(&adj);
            baseline.step(&adj);
        }
        let r_with = with_coupling.compute_order_parameter().0;
        let r_base = baseline.compute_order_parameter().0;

        println!(
            "A/B coherence evidence: initial R={:.4}, with_coupling R={:.4}, baseline R={:.4}",
            r0, r_with, r_base
        );

        // Coupling should create materially higher coherence than baseline drift.
        assert!(r_with > r_base + 0.2, "expected coupling lift >0.2");
        assert!(r_with > 0.7, "expected strong coherence with coupling");
    }
}
