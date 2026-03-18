//! Centralized configuration constants for HSM-II.
//!
//! All magic numbers, default values, and thresholds are defined here
//! so they can be referenced by name instead of scattered as literals.
//! Override at runtime via environment variables where noted.

// ── Network & Endpoints ──────────────────────────────────────────────
pub mod network {
    pub const DEFAULT_OLLAMA_HOST: &str = "http://localhost";
    pub const DEFAULT_OLLAMA_PORT: u16 = 11434;
    pub const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";

    pub const DEFAULT_ROODB_HOST: &str = "127.0.0.1";
    pub const DEFAULT_ROODB_PORT: u16 = 3307;

    pub const DEFAULT_CONDUCTOR_BIND: &str = "127.0.0.1:9001";
    pub const DEFAULT_HYPERGRAPH_BIND: &str = "127.0.0.1:8787";
    pub const DEFAULT_TEAM_API_BIND: &str = "127.0.0.1:8788";
    pub const DEFAULT_HERMES_ENDPOINT: &str = "http://localhost:8000";

    pub const HYPERGRAPH_URL: &str = "http://127.0.0.1:8787";
    pub const MONOLITH_URL: &str = "http://127.0.0.1:9000";
    pub const CONDUCTOR_URL: &str = "http://127.0.0.1:9001";
}

// ── Timeouts & Durations ─────────────────────────────────────────────
pub mod timeouts {
    use std::time::Duration;

    pub const TOOL_EXECUTION_MS: u64 = 60_000;
    pub const HTTP_REQUEST_SECS: u64 = 60;
    pub const HTTP_REQUEST_SHORT_SECS: u64 = 30;
    pub const SHELL_COMMAND_SECS: u64 = 30;
    pub const HYPERGRAPH_CLIENT_SECS: u64 = 30;
    pub const ROODB_CONNECT_SECS: u64 = 5;
    pub const AGENT_INTERVAL_MS: u64 = 1200;
    pub const USAGE_FLUSH_SECS: u64 = 300;
    pub const OPTIMIZE_INTERVAL_MS: u64 = 300_000;
    pub const METRICS_REPORT_SECS: u64 = 60;
    pub const HEARTBEAT_SECS: u64 = 1800;
    pub const LATENCY_BUDGET_MS: u64 = 60_000;

    pub fn roodb_connect() -> Duration {
        Duration::from_secs(ROODB_CONNECT_SECS)
    }
    pub fn tool_execution() -> Duration {
        Duration::from_millis(TOOL_EXECUTION_MS)
    }
    pub fn http_request() -> Duration {
        Duration::from_secs(HTTP_REQUEST_SECS)
    }
    pub fn shell_command() -> Duration {
        Duration::from_secs(SHELL_COMMAND_SECS)
    }
}

// ── Size Limits ──────────────────────────────────────────────────────
pub mod limits {
    /// 10 MB
    pub const MAX_FILE_SIZE_BYTES: usize = 10 * 1024 * 1024;
    pub const MAX_EDIT_SIZE: usize = 100_000;
    /// 100 KB
    pub const MAX_SHELL_OUTPUT_BYTES: usize = 100 * 1024;
    /// 100 MB
    pub const MAX_READ_BYTES: usize = 100 * 1024 * 1024;
    /// 256 KB
    pub const MAX_WRITE_BYTES: usize = 256 * 1024;
    /// 256 KB
    pub const MAX_EDIT_BYTES: usize = 256 * 1024;
    pub const MAX_OUTPUT_CHARS: usize = 10_000;
    pub const PREVIEW_CHARS: usize = 240;

    pub const DEFAULT_READ_LINES: usize = 2000;
    pub const DEFAULT_LOG_LIMIT: usize = 50;
    pub const DEFAULT_HISTORY_LIMIT: usize = 1000;
    pub const MAX_SEARCH_RESULTS: usize = 100;
    pub const MAX_GREP_RESULTS: usize = 100;
    pub const MAX_FIND_RESULTS: usize = 1000;
    pub const MAX_LINE_LENGTH: usize = 500;
    pub const MAX_FILES_TO_PROCESS: usize = 1000;
    pub const MAX_WEB_SEARCH_RESULTS: usize = 10;
}

// ── Thresholds & Scoring ─────────────────────────────────────────────
pub mod thresholds {
    pub const DEFAULT_TEMPERATURE: f64 = 0.7;
    pub const AGENT_TEMPERATURE: f64 = 0.2;
    pub const BRANCH_DIVERSITY_TEMP: f64 = 0.8;

    pub const COUNCIL_CONFIDENCE: f64 = 0.70;
    pub const COHERENCE_DEFAULT: f64 = 0.72;
    pub const STABILITY_DEFAULT: f64 = 0.30;
    pub const MEAN_TRUST_DEFAULT: f64 = 0.70;
    pub const COHERENCE_RISK_THRESHOLD: f64 = 0.4;
    pub const QUALITY_SUCCESS_THRESHOLD: f64 = 0.35;

    pub const INTERACTION_THRESHOLD: f64 = 0.8;
    pub const CONVERGENCE_RATE: f64 = 0.01;
}

// ── Algorithm Parameters ─────────────────────────────────────────────
pub mod algorithm {
    pub const DEFAULT_MAX_TOKENS: usize = 4096;
    pub const BRANCH_MAX_TOKENS: usize = 1024;
    pub const EMBEDDING_DIMENSION: usize = 768;
    pub const MAX_TRACES_IN_MEMORY: usize = 2048;
    pub const BELIEF_REEVALUATION_INTERVAL: usize = 50;

    pub const RLM_MAX_ITERATIONS: usize = 20;
    pub const RLM_MAX_DEPTH: usize = 3;
    pub const RLM_MAX_SUB_QUERIES: usize = 50;
    pub const RLM_TRUNCATE_LENGTH: usize = 5000;

    pub const OPTIMIZATION_MAX_ITERATIONS: usize = 6;
    pub const OPTIMIZATION_POPULATION_SIZE: usize = 4;
    pub const EVOLUTION_INTERVAL_TICKS: usize = 10;
    pub const LARS_EXPORT_INTERVAL: usize = 50;

    pub const DEFAULT_AGENT_COUNT: usize = 6;
    pub const DEFAULT_TEAM_AGENT_COUNT: usize = 10;
    pub const AUTONOMOUS_TEAM_COUNT: usize = 14;
}

// ── Security ─────────────────────────────────────────────────────────
pub mod security {
    pub const SECRET_ENV_PATTERNS: &[&str] = &["KEY", "TOKEN", "SECRET", "PASSWORD"];

    pub const SUSPICIOUS_MARKERS: &[&str] = &[
        "BEGIN PRIVATE KEY",
        "BEGIN OPENSSH PRIVATE KEY",
        "ghp_",
        "sk-",
        "xoxb-",
    ];

    pub const DANGEROUS_COMMANDS: &[&str] = &[
        "rm -rf /",
        "rm -rf ~",
        "> /dev/sda",
        "mkfs",
        "dd if=/dev/zero",
    ];
}

// ── File Paths ───────────────────────────────────────────────────────
pub mod paths {
    pub const DEFAULT_DATA_DIR: &str = "data/real";
    pub const DEFAULT_VAULT_DIR: &str = "vault";
    pub const GRAPH_STORE_FILE: &str = "world_state.ladybug.bincode";
    pub const GRAPH_STORE_WAL: &str = "world_state.ladybug.wal.bincode";
    pub const GRAPH_STORE_LOCK: &str = "world_state.ladybug.lock";
    pub const LEGACY_WORLD_STATE: &str = "world_state.bincode";
    pub const LEGACY_EMBEDDINGS: &str = "embedding_index.bincode";
}

// ── Models ───────────────────────────────────────────────────────────
pub mod models {
    pub const DEFAULT_RESOLUTION_MODEL: &str = "llama3.2";
    pub const DEFAULT_SCORER_MODEL: &str =
        "hf.co/DavidAU/OpenAi-GPT-oss-20b-HERETIC-uncensored-NEO-Imatrix-gguf:IQ4_NL";
}
