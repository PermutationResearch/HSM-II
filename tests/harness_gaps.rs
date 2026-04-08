//! Smoke tests for long-horizon harness gaps (checkpoints, bundles, prompt assembly,
//! deny list, context collapse, redaction, session JSONL, cron config discovery, bash argv vs docker).

use hyper_stigmergy::harness::{
    append_session_event, load_recent_session_events, redact_secrets, ContextTier,
    HarnessRunEnvelope, TierBudget, TierPolicy,
};
use hyper_stigmergy::personal::hsm_cron;
use hyper_stigmergy::personal::prompt_assembly::{assemble_prompt_sections, PromptAssemblyPolicy};
use hyper_stigmergy::tools::bundle::ToolBundle;
use hyper_stigmergy::tools::harness_gate::HarnessPolicyGate;
use hyper_stigmergy::tools::shell_tools::BashTool;
use hyper_stigmergy::tools::tool_permissions::ToolPermissionContext;
use hyper_stigmergy::tools::{Tool, ToolCall, ToolRegistry};
use serde_json::json;

/// Restores previous process environment for listed keys (set, unset, or replace).
struct EnvRestore {
    entries: Vec<(String, Option<String>)>,
}

impl EnvRestore {
    fn apply(pairs: &[(&str, Option<&str>)]) -> Self {
        let mut entries = Vec::new();
        for &(key, val) in pairs {
            let prev = std::env::var(key).ok();
            match val {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
            entries.push((key.to_string(), prev));
        }
        Self { entries }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, prev) in &self.entries {
            match prev {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[tokio::test]
async fn harness_long_horizon_gaps_smoke() {
    // --- 1 + 12: tool checkpoint JSONL + correlation_id + idempotency_key ---
    {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cp = tmp.path().join("tool_cp");
        std::fs::create_dir_all(&cp).expect("mkdir");
        let _env = EnvRestore::apply(&[
            (
                "HSM_HARNESS_TOOL_CHECKPOINT_DIR",
                Some(cp.to_str().unwrap()),
            ),
            ("HSM_TOOL_APPROVAL_LIST", None),
        ]);
        let mut reg = ToolRegistry::with_default_tools();
        let mut env = HarnessRunEnvelope::lead_thread("thread-gap-test");
        env.run.correlation_id = Some("corr-gap-42".into());
        reg.set_harness_context(Some(env));
        let call = ToolCall {
            name: "read".into(),
            parameters: json!({"path": "Cargo.toml"}),
            call_id: "call-1".into(),
            harness_run: None,
            idempotency_key: Some("idem-gap-9".into()),
        };
        let res = reg.execute(call).await;
        assert!(
            res.output.success,
            "read Cargo.toml: {:?}",
            res.output.error
        );
        let jpath = cp.join("tool_calls.jsonl");
        let txt = std::fs::read_to_string(&jpath).expect("read checkpoint");
        let line = txt.lines().next().expect("one line");
        let v: serde_json::Value = serde_json::from_str(line).expect("json");
        assert_eq!(v["tool"], "read");
        assert_eq!(v["correlation_id"], "corr-gap-42");
        assert_eq!(v["idempotency_key"], "idem-gap-9");
        assert!(v["params_redacted"].is_string());
    }

    // --- 3: retry env parses without breaking execute (default max_retry=0) ---
    {
        let _env = EnvRestore::apply(&[
            ("HSM_TOOL_RETRY_MAX", Some("0")),
            ("HSM_TOOL_RETRY_MS", Some("1")),
            ("HSM_TOOL_APPROVAL_LIST", None),
        ]);
        let mut reg = ToolRegistry::with_default_tools();
        let call = ToolCall {
            name: "read".into(),
            parameters: json!({"path": "Cargo.toml"}),
            call_id: "r2".into(),
            harness_run: None,
            idempotency_key: None,
        };
        let res = reg.execute(call).await;
        assert!(res.output.success);
    }

    // --- 4: prompt assembly order + cap ---
    let policy = PromptAssemblyPolicy {
        section_order: vec!["z_section".into(), "a_section".into()],
        caps: [("a_section".to_string(), 4usize)].into_iter().collect(),
    };
    let parts = vec![
        ("a_section".into(), "aaaaaa".into()),
        ("z_section".into(), "Z".into()),
    ];
    let assembled = assemble_prompt_sections(&parts, &policy);
    assert!(assembled.starts_with('Z'));
    assert!(
        assembled.contains("aaaa…"),
        "expected byte cap truncation, got {assembled:?}"
    );

    // --- 5: tool bundles ---
    {
        let _env = EnvRestore::apply(&[("HSM_TOOL_APPROVAL_LIST", None)]);
        let mut reg = ToolRegistry::with_default_tools();
        assert!(reg
            .register_bundle(ToolBundle::new(
                "core",
                "1",
                vec!["read".into(), "grep".into()]
            ))
            .is_ok());
        assert_eq!(reg.list_bundle_ids(), vec!["core"]);
        let b = reg.get_bundle("core").expect("bundle");
        assert_eq!(b.tool_names.len(), 2);
        assert!(reg
            .register_bundle(ToolBundle::new("bad", "1", vec!["nope".into()]))
            .is_err());
    }

    // --- 6: argv + docker conflict (clear error, no docker run) ---
    {
        let _env = EnvRestore::apply(&[("HSM_DOCKER_BASH", Some("1"))]);
        let bash = BashTool::new();
        let out = bash.execute(json!({ "argv": ["/bin/echo", "x"] })).await;
        assert!(!out.success);
        let err = out.error.unwrap_or_default();
        assert!(
            err.contains("HSM_DOCKER_BASH") && err.contains("argv"),
            "{err}"
        );
    }

    // --- 7: HSM_TOOL_DENY_NETWORK ---
    {
        let _env = EnvRestore::apply(&[("HSM_TOOL_DENY_NETWORK", Some("web_search"))]);
        let gate = HarnessPolicyGate::new(ToolPermissionContext::permissive());
        let r = gate.check_tool("web_search", None);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("HSM_TOOL_DENY_NETWORK"));
    }

    // --- 8: middle chat collapse (L1) ---
    {
        let policy = TierPolicy {
            chat: TierBudget::default(),
            max_inject_tier: ContextTier::L1Detail,
            chat_l1_tail_pairs: 2,
            chat_l0_pair_line_cap: 240,
            collapse_chat_middle: true,
        };
        let hist: Vec<_> = (0..8).map(|i| (format!("u{i}"), format!("a{i}"))).collect();
        let clipped = policy.clip_chat_pairs(&hist);
        assert!(
            clipped.iter().any(|(u, _)| u.contains("collapsed")),
            "{clipped:?}"
        );
    }

    // --- 9: redaction ---
    let leaked = "Authorization: Bearer secret_token_here";
    let red = redact_secrets(leaked);
    assert!(!red.contains("secret_token_here"));
    assert!(red.contains("REDACTED"));

    // --- 10: session JSONL ---
    {
        let home = tempfile::tempdir().expect("tempdir");
        let _env = EnvRestore::apply(&[("HSM_SESSION_SECRET", None)]);
        append_session_event(home.path(), "tid-1", json!({"kind": "test", "n": 1}))
            .await
            .expect("append");
        let events = load_recent_session_events(home.path(), "tid-1", 10).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["kind"], "test");
    }

    // --- 11: cron file configured ---
    {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg_path = tmp.path().join("cron.json");
        std::fs::write(&cfg_path, r#"{"jobs":[]}"#).expect("write cron");
        let _env = EnvRestore::apply(&[("HSM_CRON_CONFIG", Some(cfg_path.to_str().unwrap()))]);
        assert!(hsm_cron::cron_file_configured());
    }
}
