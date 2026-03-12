//! Multi-fractal structure analysis for DKS populations.
//!
//! Multi-fractal systems exhibit different scaling behaviors at different scales,
//! unlike simple fractals which have uniform scaling. This applies to DKS
//! populations where different sub-populations may have different persistence
//! characteristics.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Multi-fractal spectrum analysis
///
/// The multifractal spectrum f(α) describes the distribution of singularities
/// in a complex system. For DKS, this reveals how persistence varies across
/// different scales of the population.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MultifractalSpectrum {
    /// Singularity strengths (α values)
    pub alpha_values: Vec<f64>,
    /// Corresponding fractal dimensions f(α)
    pub fractal_dimensions: Vec<f64>,
    /// Moments q used for calculation
    pub moments: Vec<f64>,
    /// Generalized dimensions D(q)
    pub generalized_dimensions: Vec<f64>,
}

impl MultifractalSpectrum {
    /// Create empty spectrum
    pub fn new() -> Self {
        Self {
            alpha_values: Vec::new(),
            fractal_dimensions: Vec::new(),
            moments: Vec::new(),
            generalized_dimensions: Vec::new(),
        }
    }

    /// Calculate spectrum from persistence distribution
    ///
    /// Uses the method of moments (box-counting with varying q)
    pub fn from_persistence_distribution(persistence_values: &[f64], box_sizes: &[usize]) -> Self {
        let mut spectrum = Self::new();

        // Calculate partition functions for different q values
        let q_values: Vec<f64> = (-20..=20).map(|q| q as f64 / 4.0).collect();

        for q in &q_values {
            let mut tau_q = 0.0;

            for &box_size in box_sizes {
                let partition = Self::calculate_partition(persistence_values, box_size, *q);
                if partition > 0.0 {
                    tau_q += partition.ln() / (box_size as f64).ln();
                }
            }

            tau_q /= box_sizes.len() as f64;
            let d_q = tau_q / (1.0 - q);

            spectrum.moments.push(*q);
            spectrum.generalized_dimensions.push(d_q);
        }

        // Calculate α and f(α) via Legendre transform
        spectrum.calculate_legendre_transform();

        spectrum
    }

    /// Calculate partition function Z(q, ε) = Σ(p_i^q)
    fn calculate_partition(values: &[f64], box_size: usize, q: f64) -> f64 {
        let mut boxes: HashMap<usize, f64> = HashMap::new();

        // Bin values into boxes
        for (i, &value) in values.iter().enumerate() {
            let box_idx = i / box_size;
            *boxes.entry(box_idx).or_insert(0.0) += value;
        }

        // Calculate partition sum
        let total: f64 = boxes.values().sum();
        if total == 0.0 {
            return 0.0;
        }

        boxes
            .values()
            .map(|&p| {
                let prob = p / total;
                if q == 1.0 {
                    prob * prob.ln()
                } else {
                    prob.powf(q)
                }
            })
            .sum()
    }

    /// Calculate α and f(α) via Legendre transform
    ///
    /// α(q) = dτ/dq
    /// f(α) = q*α - τ(q)
    fn calculate_legendre_transform(&mut self) {
        for i in 0..self.moments.len() {
            let q = self.moments[i];
            let d_q = self.generalized_dimensions[i];

            // Approximate α as D(q) + q * dD/dq
            let alpha = if i > 0 && i < self.moments.len() - 1 {
                let dq = self.moments[i + 1] - self.moments[i - 1];
                let dd = self.generalized_dimensions[i + 1] - self.generalized_dimensions[i - 1];
                d_q + q * dd / dq
            } else {
                d_q
            };

            // f(α) = q*α - τ(q) = q*α - D(q)*(1-q)
            let f_alpha = q * alpha - d_q * (1.0 - q);

            self.alpha_values.push(alpha);
            self.fractal_dimensions.push(f_alpha);
        }
    }

    /// Get the width of the spectrum (α_max - α_min)
    ///
    /// Wider spectrum indicates more heterogeneous structure
    pub fn width(&self) -> f64 {
        if self.alpha_values.is_empty() {
            return 0.0;
        }
        let max_alpha = self
            .alpha_values
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        let min_alpha = self
            .alpha_values
            .iter()
            .cloned()
            .fold(f64::INFINITY, f64::min);
        max_alpha - min_alpha
    }

    /// Check if system is multifractal (width > threshold)
    pub fn is_multifractal(&self, threshold: f64) -> bool {
        self.width() > threshold
    }

    /// Get maximum fractal dimension (capacity dimension)
    pub fn capacity_dimension(&self) -> f64 {
        self.fractal_dimensions.iter().cloned().fold(0.0, f64::max)
    }
}

impl Default for MultifractalSpectrum {
    fn default() -> Self {
        Self::new()
    }
}

/// Multi-scale DKS analysis
///
/// Analyzes DKS populations at different scales to reveal hierarchical structure
pub struct MultiscaleDKS {
    scales: Vec<ScaleAnalysis>,
}

#[derive(Clone, Debug)]
pub struct ScaleAnalysis {
    pub scale: usize,
    pub population_size: usize,
    pub avg_persistence: f64,
    pub persistence_variance: f64,
    pub dks_stability: f64,
    pub spectrum: MultifractalSpectrum,
}

impl MultiscaleDKS {
    pub fn new() -> Self {
        Self { scales: Vec::new() }
    }

    /// Get the scale analyses
    pub fn scales(&self) -> &[ScaleAnalysis] {
        &self.scales
    }

    /// Analyze population at multiple scales
    pub fn analyze(&mut self, persistence_values: &[f64]) {
        let scales: Vec<usize> = vec![1, 2, 4, 8, 16, 32, 64];

        for scale in scales {
            if scale > persistence_values.len() {
                break;
            }

            let analysis = self.analyze_scale(persistence_values, scale);
            self.scales.push(analysis);
        }
    }

    fn analyze_scale(&self, values: &[f64], scale: usize) -> ScaleAnalysis {
        // Coarse-grain the data
        let coarse: Vec<f64> = values
            .chunks(scale)
            .map(|chunk| chunk.iter().sum::<f64>() / chunk.len() as f64)
            .collect();

        let n = coarse.len();
        let avg = coarse.iter().sum::<f64>() / n as f64;
        let variance = coarse.iter().map(|&x| (x - avg).powi(2)).sum::<f64>() / n as f64;

        // Calculate DKS stability for this scale
        let replication = avg.max(0.0);
        let decay = (1.0 - avg).max(0.01);
        let stability = super::calculate_dks_stability(replication, decay);

        // Calculate multifractal spectrum
        let box_sizes: Vec<usize> = (1..=5).map(|i| 2usize.pow(i)).collect();
        let spectrum = MultifractalSpectrum::from_persistence_distribution(&coarse, &box_sizes);

        ScaleAnalysis {
            scale,
            population_size: n,
            avg_persistence: avg,
            persistence_variance: variance,
            dks_stability: stability,
            spectrum,
        }
    }

    /// Get scaling exponents across scales
    pub fn scaling_exponents(&self) -> Vec<(usize, f64)> {
        self.scales
            .iter()
            .map(|s| (s.scale, s.dks_stability))
            .collect()
    }

    /// Detect critical scale where behavior changes
    pub fn critical_scale(&self) -> Option<usize> {
        // Find where variance peaks or stability crosses threshold
        self.scales
            .iter()
            .max_by(|a, b| {
                a.persistence_variance
                    .partial_cmp(&b.persistence_variance)
                    .unwrap()
            })
            .map(|s| s.scale)
    }
}

impl Default for MultiscaleDKS {
    fn default() -> Self {
        Self::new()
    }
}

/// Kolmogorov complexity-inspired compositionality measure
///
/// Measures how much the whole population's structure exceeds the sum of its parts
pub fn compositionality_measure(
    whole_description_length: f64,
    part_description_lengths: &[f64],
) -> f64 {
    let sum_parts: f64 = part_description_lengths.iter().sum();

    // K(m_x | m_xa, m_xb) > 0 indicates compositional structure
    // Return the excess complexity
    (whole_description_length - sum_parts).max(0.0)
}
