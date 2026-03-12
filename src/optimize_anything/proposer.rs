//! LLM Proposer for generating improved artifacts

use super::{ASI, Artifact, Candidate};
use ollama_rs::{Ollama, generation::chat::{ChatMessage, ChatMessageRequest, MessageRole}};

/// LLM-based proposer for artifact improvement
pub struct LLMProposer {
    model: String,
    temperature: f32,
    ollama: Ollama,
}

impl LLMProposer {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            temperature: 0.7,
            ollama: Ollama::new("http://localhost".to_string(), 11434),
        }
    }

    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = temp.clamp(0.0, 2.0);
        self
    }

    /// Propose an improved artifact based on a parent candidate
    pub async fn propose_improvement(
        &self,
        parent: &Candidate,
        objective: &str,
    ) -> anyhow::Result<Artifact> {
        let system_prompt = format!(
            "You are an expert optimizer. Improve the given artifact based on the objective and feedback.\n\
             Objective: {}\n\
             Current score: {:.2}\n\
             Be specific and make concrete improvements.",
            objective, parent.score
        );

        let asi_text = if parent.asi.text.is_empty() {
            "No specific feedback available.".to_string()
        } else {
            parent.asi.text.join("\n")
        };

        let user_prompt = format!(
            "Current artifact:\n```\n{}\n```\n\n\
             Feedback/ASI:\n{}\n\n\
             Provide an improved version that addresses the issues and achieves a higher score.\n\
             Respond with ONLY the improved artifact, no commentary.",
            parent.artifact.content, asi_text
        );

        let messages = vec![
            ChatMessage::new(MessageRole::System, system_prompt),
            ChatMessage::new(MessageRole::User, user_prompt),
        ];

        let request = ChatMessageRequest::new(self.model.clone(), messages);
        
        match self.ollama.send_chat_messages(request).await {
            Ok(response) => {
                let improved = response.message.content.trim().to_string();
                Ok(Artifact::new(improved))
            }
            Err(e) => Err(anyhow::anyhow!("LLM proposer failed: {}", e)),
        }
    }

    /// Propose a new artifact from scratch (seedless optimization)
    pub async fn propose_seed(
        &self,
        objective: &str,
        background: &str,
    ) -> anyhow::Result<Artifact> {
        let system_prompt = format!(
            "You are an expert creator. Design an artifact that satisfies the objective.\n\
             Background context: {}",
            background
        );

        let user_prompt = format!(
            "Objective: {}\n\n\
             Create an initial artifact that satisfies this objective.\n\
             Respond with ONLY the artifact content, no commentary.",
            objective
        );

        let messages = vec![
            ChatMessage::new(MessageRole::System, system_prompt),
            ChatMessage::new(MessageRole::User, user_prompt),
        ];

        let request = ChatMessageRequest::new(self.model.clone(), messages);
        
        match self.ollama.send_chat_messages(request).await {
            Ok(response) => {
                let content = response.message.content.trim().to_string();
                Ok(Artifact::new(content))
            }
            Err(e) => Err(anyhow::anyhow!("LLM proposer failed: {}", e)),
        }
    }

    /// Reflect on ASI and propose targeted improvements
    pub async fn propose_with_reflection(
        &self,
        candidates: &[Candidate],
        objective: &str,
    ) -> anyhow::Result<Artifact> {
        if candidates.is_empty() {
            return self.propose_seed(objective, "").await;
        }

        // Sort by score
        let mut sorted = candidates.to_vec();
        sorted.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        let best = &sorted[0];
        let worst = sorted.last().unwrap();

        let system_prompt = format!(
            "You are an expert optimizer using reflection. Analyze what worked and what didn't.\n\
             Objective: {}",
            objective
        );

        let user_prompt = format!(
            "Best candidate (score {:.2}):\n```\n{}\n```\n\n\
             Worst candidate (score {:.2}):\n```\n{}\n```\n\n\
             Reflection:\n\
             1. What makes the best candidate better?\n\
             2. What are the key failure patterns?\n\
             3. Create an improved artifact that combines the best aspects and avoids failures.\n\n\
             Respond with ONLY the improved artifact, no commentary.",
            best.score, best.artifact.content,
            worst.score, worst.artifact.content
        );

        let messages = vec![
            ChatMessage::new(MessageRole::System, system_prompt),
            ChatMessage::new(MessageRole::User, user_prompt),
        ];

        let request = ChatMessageRequest::new(self.model.clone(), messages);
        
        match self.ollama.send_chat_messages(request).await {
            Ok(response) => {
                let content = response.message.content.trim().to_string();
                Ok(Artifact::new(content))
            }
            Err(e) => Err(anyhow::anyhow!("LLM proposer failed: {}", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proposer_builder() {
        let proposer = LLMProposer::new("test-model")
            .with_temperature(0.5);
        
        assert_eq!(proposer.model, "test-model");
        assert!((proposer.temperature - 0.5).abs() < 0.01);
    }
}
