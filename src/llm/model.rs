//! Model loading and management.

/// Model types supported
#[derive(Clone, Debug)]
pub enum ModelType {
    Llama,
    Mistral,
    Phi,
    Qwen,
    Custom(String),
}

impl ModelType {
    pub fn from_id(model_id: &str) -> Self {
        if model_id.contains("llama") {
            ModelType::Llama
        } else if model_id.contains("mistral") {
            ModelType::Mistral
        } else if model_id.contains("phi") {
            ModelType::Phi
        } else if model_id.contains("qwen") {
            ModelType::Qwen
        } else {
            ModelType::Custom(model_id.to_string())
        }
    }

    pub fn default_hidden_size(&self) -> usize {
        match self {
            ModelType::Llama => 4096,
            ModelType::Mistral => 4096,
            ModelType::Phi => 2560,
            ModelType::Qwen => 4096,
            ModelType::Custom(_) => 4096,
        }
    }

    pub fn default_num_layers(&self) -> usize {
        match self {
            ModelType::Llama => 32,
            ModelType::Mistral => 32,
            ModelType::Phi => 32,
            ModelType::Qwen => 32,
            ModelType::Custom(_) => 32,
        }
    }
}

/// Model loader
pub struct ModelLoader {
    model_type: ModelType,
    cache_dir: std::path::PathBuf,
}

impl ModelLoader {
    pub fn new(model_id: &str) -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from(".cache"))
            .join("hyper-stigmergy")
            .join("models");

        Self {
            model_type: ModelType::from_id(model_id),
            cache_dir,
        }
    }

    pub fn with_cache_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.cache_dir = dir;
        self
    }

    /// Check if model is cached locally
    pub fn is_cached(&self, model_id: &str) -> bool {
        let model_path = self.cache_dir.join(model_id.replace('/', "--"));
        model_path.exists()
    }

    /// Download model if not cached
    pub async fn download(&self, model_id: &str) -> anyhow::Result<std::path::PathBuf> {
        let model_path = self.cache_dir.join(model_id.replace('/', "--"));

        if model_path.exists() {
            return Ok(model_path);
        }

        // In production, would download from HuggingFace
        // For now, create placeholder
        std::fs::create_dir_all(&model_path)?;

        Ok(model_path)
    }

    /// Get model info
    pub fn model_info(&self) -> ModelInfo {
        ModelInfo {
            model_type: self.model_type.clone(),
            hidden_size: self.model_type.default_hidden_size(),
            num_layers: self.model_type.default_num_layers(),
            vocab_size: 32000,
        }
    }
}

/// Model information
#[derive(Clone, Debug)]
pub struct ModelInfo {
    pub model_type: ModelType,
    pub hidden_size: usize,
    pub num_layers: usize,
    pub vocab_size: usize,
}

/// Quantization levels
#[derive(Clone, Copy, Debug)]
pub enum Quantization {
    None,
    Q4_0, // 4-bit, no block quantization
    Q4_1, // 4-bit, with block quantization
    Q5_0,
    Q5_1,
    Q8_0, // 8-bit
}

impl Quantization {
    pub fn bits_per_weight(&self) -> f32 {
        match self {
            Quantization::None => 16.0, // F16
            Quantization::Q4_0 | Quantization::Q4_1 => 4.0,
            Quantization::Q5_0 | Quantization::Q5_1 => 5.0,
            Quantization::Q8_0 => 8.0,
        }
    }

    pub fn memory_reduction(&self) -> f32 {
        16.0 / self.bits_per_weight()
    }
}

/// Model weights container
pub struct ModelWeights {
    pub info: ModelInfo,
    pub quantization: Quantization,
    pub tensors: Vec<Tensor>,
}

pub struct Tensor {
    pub name: String,
    pub shape: Vec<usize>,
    pub data: Vec<u8>,
    pub dtype: DType,
}

#[derive(Clone, Copy, Debug)]
pub enum DType {
    F32,
    F16,
    Q4_0,
    Q4_1,
    Q5_0,
    Q5_1,
    Q8_0,
}
