//! Token encoding/decoding.

/// Token encoder
pub struct TokenEncoder {
    _vocab: Vec<String>,
    _special_tokens: std::collections::HashMap<String, usize>,
}

impl TokenEncoder {
    pub fn new() -> Self {
        let mut special_tokens = std::collections::HashMap::new();
        special_tokens.insert("<|endoftext|>".to_string(), 0);
        special_tokens.insert("<|im_start|>".to_string(), 1);
        special_tokens.insert("<|im_end|>".to_string(), 2);

        Self {
            _vocab: Vec::new(),
            _special_tokens: special_tokens,
        }
    }

    /// Encode text to token IDs
    pub fn encode(&self, text: &str, _options: EncodingOptions) -> Vec<usize> {
        // Simple word-based tokenization for placeholder
        // In production, would use BPE or SentencePiece
        let mut tokens = Vec::new();

        for word in text.split_whitespace() {
            // Hash the word to get a token ID
            let mut hash: usize = 0;
            for byte in word.bytes() {
                hash = hash.wrapping_mul(31).wrapping_add(byte as usize);
            }
            tokens.push(hash % 30000 + 100); // Reserve first 100 for special tokens
        }

        tokens
    }

    /// Decode token IDs to text
    pub fn decode(&self, tokens: &[usize]) -> String {
        // Placeholder: just join token IDs
        tokens
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Count tokens in text
    pub fn count_tokens(&self, text: &str) -> usize {
        self.encode(text, EncodingOptions::default()).len()
    }

    /// Get vocabulary size
    pub fn vocab_size(&self) -> usize {
        32000
    }
}

impl Default for TokenEncoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Encoding options
#[derive(Clone, Debug)]
pub struct EncodingOptions {
    pub add_special_tokens: bool,
    pub max_length: Option<usize>,
    pub truncation: bool,
    pub padding: bool,
}

impl Default for EncodingOptions {
    fn default() -> Self {
        Self {
            add_special_tokens: true,
            max_length: None,
            truncation: false,
            padding: false,
        }
    }
}

/// Chat template formatter
pub struct ChatTemplate {
    template: String,
}

impl ChatTemplate {
    pub fn new(template: &str) -> Self {
        Self {
            template: template.to_string(),
        }
    }

    /// Format chat messages into prompt
    pub fn format(&self, messages: &[ChatMessage]) -> String {
        let mut result = self.template.clone();

        for (i, msg) in messages.iter().enumerate() {
            let placeholder = format!("{{message{}}}", i);
            result = result.replace(&placeholder, &msg.content);
        }

        result
    }

    /// Default Llama-2 chat template
    pub fn llama2() -> Self {
        Self::new("<s>[INST] {{system_prompt}} {{user_message}} [/INST]")
    }

    /// Default Mistral chat template
    pub fn mistral() -> Self {
        Self::new("<s>[INST] {{system_prompt}} {{user_message}} [/INST]")
    }
}

/// Chat message
#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Clone, Debug)]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

impl ChatRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChatRole::System => "system",
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
        }
    }
}
