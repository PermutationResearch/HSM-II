use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TurnRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTurn {
    pub role: TurnRole,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub meta: HashMap<String, String>,
}

impl SessionTurn {
    pub fn new(role: TurnRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            created_at: Utc::now(),
            meta: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub id: Uuid,
    pub turn_count: usize,
    pub turns: Vec<SessionTurn>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub max_turns: usize,
    pub snapshot_every_turns: usize,
    pub include_history_by_default: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_turns: 30,
            snapshot_every_turns: 4,
            include_history_by_default: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationExample {
    pub input: String,
    pub output: String,
    pub context: String,
}

#[async_trait]
pub trait DspySessionAdapter: Send + Sync {
    async fn forward(&self, input: &str, history: &[SessionTurn]) -> Result<String>;
}

pub struct DspySession<A: DspySessionAdapter> {
    adapter: A,
    config: SessionConfig,
    id: Uuid,
    turns: Vec<SessionTurn>,
    snapshots: Vec<SessionSnapshot>,
}

impl<A: DspySessionAdapter> DspySession<A> {
    pub fn new(adapter: A) -> Self {
        Self::with_config(adapter, SessionConfig::default())
    }

    pub fn with_config(adapter: A, config: SessionConfig) -> Self {
        Self {
            adapter,
            config,
            id: Uuid::new_v4(),
            turns: Vec::new(),
            snapshots: Vec::new(),
        }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn turns(&self) -> &[SessionTurn] {
        &self.turns
    }

    pub fn snapshots(&self) -> &[SessionSnapshot] {
        &self.snapshots
    }

    pub async fn forward(
        &mut self,
        user_input: impl Into<String>,
        include_history: Option<bool>,
    ) -> Result<String> {
        let user_input = user_input.into();
        self.turns
            .push(SessionTurn::new(TurnRole::User, user_input.clone()));

        let include_history = include_history.unwrap_or(self.config.include_history_by_default);
        let history_buf;
        let history = if include_history {
            self.turns.as_slice()
        } else {
            history_buf = vec![SessionTurn::new(TurnRole::User, user_input.clone())];
            history_buf.as_slice()
        };

        let output = self.adapter.forward(&user_input, history).await?;
        self.turns
            .push(SessionTurn::new(TurnRole::Assistant, output.clone()));

        self.trim_turns();
        self.maybe_snapshot();
        Ok(output)
    }

    pub fn add_turn(&mut self, role: TurnRole, content: impl Into<String>) {
        self.turns.push(SessionTurn::new(role, content));
        self.trim_turns();
        self.maybe_snapshot();
    }

    pub fn snapshot_now(&mut self) -> SessionSnapshot {
        let snapshot = SessionSnapshot {
            id: Uuid::new_v4(),
            turn_count: self.turns.len(),
            turns: self.turns.clone(),
            created_at: Utc::now(),
        };
        self.snapshots.push(snapshot.clone());
        snapshot
    }

    pub fn as_linear_text(&self) -> String {
        let mut out = String::new();
        for turn in &self.turns {
            let role = match turn.role {
                TurnRole::User => "User",
                TurnRole::Assistant => "Assistant",
            };
            out.push_str(role);
            out.push_str(": ");
            out.push_str(&turn.content);
            out.push('\n');
        }
        out.trim_end().to_string()
    }

    pub fn to_optimization_examples(&self) -> Vec<OptimizationExample> {
        let mut examples = Vec::new();
        for i in 0..self.turns.len() {
            if self.turns[i].role != TurnRole::Assistant || i == 0 {
                continue;
            }
            if self.turns[i - 1].role != TurnRole::User {
                continue;
            }
            let input = self.turns[i - 1].content.clone();
            let output = self.turns[i].content.clone();
            let context = self.turns[..i - 1]
                .iter()
                .map(|t| {
                    let role = match t.role {
                        TurnRole::User => "User",
                        TurnRole::Assistant => "Assistant",
                    };
                    format!("{role}: {}", t.content)
                })
                .collect::<Vec<_>>()
                .join("\n");
            examples.push(OptimizationExample {
                input,
                output,
                context,
            });
        }
        examples
    }

    fn trim_turns(&mut self) {
        if self.turns.len() <= self.config.max_turns {
            return;
        }
        let keep_from = self.turns.len() - self.config.max_turns;
        self.turns.drain(0..keep_from);
    }

    fn maybe_snapshot(&mut self) {
        if self.config.snapshot_every_turns == 0 {
            return;
        }
        if self.turns.len() % self.config.snapshot_every_turns == 0 {
            self.snapshot_now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoAdapter;

    #[async_trait]
    impl DspySessionAdapter for EchoAdapter {
        async fn forward(&self, input: &str, history: &[SessionTurn]) -> Result<String> {
            Ok(format!("echo:{input} hist:{}", history.len()))
        }
    }

    #[tokio::test]
    async fn forwards_and_captures_turns() {
        let mut session = DspySession::with_config(
            EchoAdapter,
            SessionConfig {
                max_turns: 10,
                snapshot_every_turns: 2,
                include_history_by_default: true,
            },
        );
        let out = session.forward("hello", None).await.expect("forward");
        assert!(out.contains("echo:hello"));
        assert_eq!(session.turns().len(), 2);
        assert_eq!(session.snapshots().len(), 1);
    }

    #[tokio::test]
    async fn creates_examples_from_user_assistant_pairs() {
        let mut session = DspySession::new(EchoAdapter);
        let _ = session
            .forward("q1", Some(false))
            .await
            .expect("forward q1");
        let _ = session
            .forward("q2", Some(false))
            .await
            .expect("forward q2");
        let examples = session.to_optimization_examples();
        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0].input, "q1");
    }
}
