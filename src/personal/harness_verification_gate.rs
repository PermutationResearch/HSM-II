//! Hard verification gate for the Hermes native tool loop (`EnhancedPersonalAgent`).
//!
//! Enable with `HSM_HARNESS_VERIFICATION_HARD=1`. Requires successful `read` or `grep` plus
//! successful `bash` with substantive output before a plain-text final answer is accepted.
//! When enabled, **any** final text without that evidence (including zero tool calls) is replaced
//! with `[HARD VERIFICATION GATE]`.

use crate::trace2skill::ToolStepRecord;

pub fn harness_verification_hard_enabled() -> bool {
    std::env::var("HSM_HARNESS_VERIFICATION_HARD")
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

/// Returns true when tool trace shows evidence-style usage (read|grep + bash proof).
pub fn passes_hard_verification_gate(steps: &[ToolStepRecord]) -> bool {
    let mut has_read_or_grep = false;
    let mut has_evidence_bash = false;
    for s in steps {
        if !s.ok {
            continue;
        }
        match s.name.as_str() {
            "read" | "grep" => has_read_or_grep = true,
            "bash" => {
                let sum = s.result_summary.to_ascii_lowercase();
                if sum.contains("exit code 0")
                    || sum.contains("command completed")
                    || sum.contains("ls ")
                    || sum.contains("cargo ")
                    || sum.contains("total ")
                    || s.result_summary.len() > 120
                {
                    has_evidence_bash = true;
                }
            }
            _ => {}
        }
    }
    has_read_or_grep && has_evidence_bash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(name: &str, ok: bool, summary: &str) -> ToolStepRecord {
        ToolStepRecord {
            name: name.to_string(),
            args_redacted: "{}".into(),
            ok,
            result_summary: summary.into(),
        }
    }

    #[test]
    fn gate_requires_read_and_bash() {
        let steps = vec![
            step("read", true, "file contents"),
            step(
                "bash",
                true,
                "Command completed (exit code 0)\n\nstdout:\nok\n",
            ),
        ];
        assert!(passes_hard_verification_gate(&steps));
    }

    #[test]
    fn gate_fails_without_read() {
        let steps = vec![step(
            "bash",
            true,
            "Command completed (exit code 0)\n\nstdout:\nok\n",
        )];
        assert!(!passes_hard_verification_gate(&steps));
    }

    #[test]
    fn gate_fails_on_empty_trace() {
        assert!(!passes_hard_verification_gate(&[]));
    }

    #[test]
    fn gate_accepts_grep_instead_of_read() {
        let long = "x".repeat(130);
        let steps = vec![
            step("grep", true, "matches"),
            step("bash", true, &long),
        ];
        assert!(passes_hard_verification_gate(&steps));
    }
}
