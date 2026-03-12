use nalgebra::DMatrix;
use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hypergraph {
    pub num_vertices: usize,
    pub hyperedges: Vec<Vec<usize>>,
    pub edge_weights: Vec<f32>,
}

/// Hypergraph convolutional layer.
///
/// Implements message-passing over hyperedges with a learnable linear
/// projection from `input_dim` to `output_dim`:
///
///   1. For each hyperedge, compute the mean feature of its member vertices.
///   2. Scatter that mean back to each member, scaled by edge weight.
///   3. Project the aggregated features through a weight matrix W.
///   4. Apply ReLU activation.
///
/// Weight matrix is Xavier-initialised in `new()`.
pub struct HypergraphConv {
    input_dim: usize,
    output_dim: usize,
    weight: DMatrix<f32>, // shape: [input_dim, output_dim]
}

impl HypergraphConv {
    pub fn new(input_dim: usize, output_dim: usize) -> Self {
        // Xavier / Glorot uniform initialisation
        let limit = (6.0 / (input_dim + output_dim) as f32).sqrt();
        let mut rng = rand::thread_rng();
        let weight = DMatrix::from_fn(input_dim, output_dim, |_, _| rng.gen_range(-limit..limit));
        Self {
            input_dim,
            output_dim,
            weight,
        }
    }

    /// Run one hypergraph convolution pass.
    ///
    /// `features` shape: [num_vertices, input_dim]
    /// Returns:          [num_vertices, output_dim]
    pub fn forward(&self, graph: &Hypergraph, features: &DMatrix<f32>) -> DMatrix<f32> {
        assert_eq!(
            features.ncols(),
            self.input_dim,
            "HypergraphConv: expected input_dim={}, got feature cols={}",
            self.input_dim,
            features.ncols()
        );

        let n = features.nrows();

        // Step 1+2: Hyperedge message passing — aggregate neighbour means
        let mut aggregated = features.clone();

        for (edge, &weight) in graph.hyperedges.iter().zip(graph.edge_weights.iter()) {
            // Collect valid vertex indices for this hyperedge
            let members: Vec<usize> = edge.iter().copied().filter(|&v| v < n).collect();
            if members.len() < 2 {
                continue;
            }

            // Compute mean feature across hyperedge members
            let inv = 1.0 / members.len() as f32;
            let mut mean = DMatrix::zeros(1, self.input_dim);
            for &v in &members {
                mean += features.row(v);
            }
            mean *= inv;

            // Scatter mean back to each member, scaled by edge weight
            for &v in &members {
                for j in 0..self.input_dim {
                    aggregated[(v, j)] += mean[(0, j)] * weight;
                }
            }
        }

        // Step 3: Linear projection  aggregated * W  →  [n, output_dim]
        let projected = &aggregated * &self.weight;

        // Step 4: ReLU activation
        DMatrix::from_fn(n, self.output_dim, |i, j| projected[(i, j)].max(0.0))
    }

    pub fn input_dim(&self) -> usize {
        self.input_dim
    }

    pub fn output_dim(&self) -> usize {
        self.output_dim
    }

    /// Access the weight matrix (e.g. for serialization or inspection).
    pub fn weight(&self) -> &DMatrix<f32> {
        &self.weight
    }

    /// Replace the weight matrix (e.g. after loading from checkpoint).
    pub fn set_weight(&mut self, w: DMatrix<f32>) {
        assert_eq!(w.nrows(), self.input_dim);
        assert_eq!(w.ncols(), self.output_dim);
        self.weight = w;
    }
}
