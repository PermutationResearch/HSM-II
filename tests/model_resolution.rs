//! Comprehensive integration test: verify every HSM-II component
//! respects the OLLAMA_MODEL environment variable.
//!
//! Run: OLLAMA_MODEL="qwen3-coder:480b-cloud" cargo test --test model_resolution
//!
//! NOTE: These tests modify process-wide env vars, so they MUST run
//! single-threaded to avoid races. The `serial_test` approach below
//! ensures ordering without requiring an external crate.

use ::hyper_stigmergy::*;
use std::sync::Mutex;

const TARGET: &str = "qwen3-coder:480b-cloud";

/// Global lock so env-mutating tests don't race each other.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Helper: assert model field == TARGET
macro_rules! assert_model {
    ($label:expr, $expr:expr) => {{
        let val = $expr;
        assert_eq!(
            val, TARGET,
            "❌ {} resolved to '{}', expected '{}'",
            $label, val, TARGET
        );
    }};
}

#[test]
fn all_components_respect_ollama_model_env() {
    let _lock = ENV_LOCK.lock().unwrap();
    std::env::set_var("OLLAMA_MODEL", TARGET);

    // ── 1. Core resolver ───────────────────────────────────────
    assert_model!(
        "resolve_model_from_env()",
        ollama_client::resolve_model_from_env("should-not-see-this")
    );

    // ── 2. OllamaConfig ────────────────────────────────────────
    let cfg = ollama_client::OllamaConfig::default();
    assert_model!("OllamaConfig::default().model", cfg.model);

    // ── 3. ScenarioSimulator ────────────────────────────────────
    let sim = scenario_simulator::ScenarioSimulatorConfig::default();
    assert_model!("ScenarioSimulatorConfig.model", sim.model);

    // ── 4. OptimizationConfig ───────────────────────────────────
    let opt = optimize_anything::OptimizationConfig::default();
    assert_model!("OptimizationConfig.model", opt.model);

    // ── 5. BatchConfig ──────────────────────────────────────────
    let batch = batch_runner::BatchConfig::default();
    assert_model!("BatchConfig.ollama_model", batch.ollama_model);

    // ── 6. LoopConfig ───────────────────────────────────────────
    let lc = loop_main::LoopConfig::default();
    assert_model!("LoopConfig.model", lc.model);

    // ── 7. Coder ProviderConfig ─────────────────────────────────
    let coder = coder_assistant::ProviderConfig::default();
    assert_model!("coder_assistant::ProviderConfig.model", coder.model);

    // ── 8. agent_core::ModelConfig ──────────────────────────────
    let ac = agent_core::ModelConfig::default();
    assert_model!("agent_core::ModelConfig.model", ac.model);

    // ── 9. Pi AI compat ─────────────────────────────────────────
    let pi = pi_ai_compat::Model::deepseek_abliterated();
    assert_model!("pi_ai_compat::Model::deepseek_abliterated().name", pi.name);

    // ── 10. RLM v2 LlmBridgeConfig ─────────────────────────────
    let rlm = rlm_v2::llm_bridge::LlmBridgeConfig::default();
    assert_model!("rlm_v2::LlmBridgeConfig.model", rlm.model);

    // ── 11. Ralph Council worker ────────────────────────────────
    let rw = council::ralph::AgentConfig::default_worker();
    assert_model!("ralph::AgentConfig::default_worker().model", rw.model);

    // ── 12. Ralph Council reviewer ──────────────────────────────
    let rr = council::ralph::AgentConfig::default_reviewer();
    assert_model!("ralph::AgentConfig::default_reviewer().model", rr.model);

    // ── 13. Coder agent loop ────────────────────────────────────
    let cal = coder_assistant::agent_loop::AgentConfig::default();
    assert_model!("coder_assistant::agent_loop::AgentConfig.model", cal.model);

    eprintln!("\n  🎉 All 13 components resolve to '{}'\n", TARGET);
}

#[test]
fn fallback_works_when_env_unset() {
    let _lock = ENV_LOCK.lock().unwrap();
    std::env::remove_var("OLLAMA_MODEL");

    let val = ollama_client::resolve_model_from_env("my-fallback");
    assert_eq!(val, "my-fallback", "Should use fallback when env unset");

    // Restore
    std::env::set_var("OLLAMA_MODEL", TARGET);
}

#[test]
fn auto_value_treated_as_unset() {
    let _lock = ENV_LOCK.lock().unwrap();
    std::env::set_var("OLLAMA_MODEL", "auto");

    let val = ollama_client::resolve_model_from_env("expected-fallback");
    assert_eq!(
        val, "expected-fallback",
        "OLLAMA_MODEL='auto' should use fallback"
    );

    // Restore
    std::env::set_var("OLLAMA_MODEL", TARGET);
}

#[test]
fn empty_env_uses_fallback() {
    let _lock = ENV_LOCK.lock().unwrap();
    std::env::set_var("OLLAMA_MODEL", "");

    let val = ollama_client::resolve_model_from_env("expected-fallback");
    assert_eq!(
        val, "expected-fallback",
        "OLLAMA_MODEL='' should use fallback"
    );

    // Restore
    std::env::set_var("OLLAMA_MODEL", TARGET);
}
