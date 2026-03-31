//! Runtime control-plane configuration for approvals, plugins, and resume storage.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct ApprovalConfig {
    pub store_path: PathBuf,
    pub interactive: bool,
}

#[derive(Clone, Debug)]
pub struct PluginConfig {
    pub manifest_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub allow_unsigned: bool,
}

#[derive(Clone, Debug)]
pub struct ResumeConfig {
    pub checkpoint_dir: PathBuf,
    pub session_map_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub state_dir: PathBuf,
    pub approvals: ApprovalConfig,
    pub plugins: PluginConfig,
    pub resume: ResumeConfig,
}

impl RuntimeConfig {
    pub fn from_env() -> Self {
        let state_dir = std::env::var("HSM_RUNTIME_STATE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(".hsmii/runtime"));
        let checkpoint_dir = std::env::var("HSM_HARNESS_CHECKPOINT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| state_dir.join("checkpoints"));
        Self {
            approvals: ApprovalConfig {
                store_path: std::env::var("HSM_APPROVAL_STORE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| state_dir.join("approvals.json")),
                interactive: std::env::var("HSM_APPROVAL_INTERACTIVE")
                    .ok()
                    .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
                    .unwrap_or(true),
            },
            plugins: PluginConfig {
                manifest_dir: std::env::var("HSM_PLUGIN_MANIFEST_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| state_dir.join("plugins")),
                cache_dir: std::env::var("HSM_PLUGIN_CACHE_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| state_dir.join("plugin_cache")),
                allow_unsigned: std::env::var("HSM_PLUGIN_ALLOW_UNSIGNED")
                    .ok()
                    .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
                    .unwrap_or(false),
            },
            resume: ResumeConfig {
                checkpoint_dir,
                session_map_path: std::env::var("HSM_RESUME_SESSION_MAP")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| state_dir.join("resume_sessions.json")),
            },
            state_dir,
        }
    }
}
