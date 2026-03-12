use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum Action {
    WriteManifesto {
        file_name: String,
        content: String,
        reason: String,
    },
    AddMemory {
        id: usize,
        text: String,
    },
    AddAgent {
        id: usize,
        name: String,
    },
    AddTool {
        id: usize,
        name: String,
    },
    AddTask {
        id: usize,
        description: String,
    },
    DescribeAgent {
        agent_id: u64,
        summary: String,
        properties: Vec<String>,
    },
    LinkAgents {
        vertices: Vec<usize>,
        weight: f32,
    },
    LinkTaskAgents {
        task: usize,
        agents: Vec<u64>,
        weight: f32,
    },
    AbliterateSubspace {
        target_id: Option<u64>,
        intensity: f64,
    },
    Noop {
        reason: String,
    },
}
