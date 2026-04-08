//! Lightweight secret redaction for logs, webhooks, and checkpoints (gap 9).

use regex::Regex;
use std::sync::OnceLock;

fn pat_bearer() -> &'static Regex {
    static P: OnceLock<Regex> = OnceLock::new();
    P.get_or_init(|| Regex::new(r"(?i)(Bearer\s+)[A-Za-z0-9._\-~+/]+=*").unwrap())
}

fn pat_sk() -> &'static Regex {
    static P: OnceLock<Regex> = OnceLock::new();
    P.get_or_init(|| Regex::new(r"(?i)(sk-[A-Za-z0-9]{20,})").unwrap())
}

fn pat_kv() -> &'static Regex {
    static P: OnceLock<Regex> = OnceLock::new();
    P.get_or_init(|| {
        Regex::new(
            r#"(?i)(api[_-]?key|token|password|secret)([\"']?\s*[:=]\s*)([\"']?)([^\s\"']{8,})"#,
        )
        .unwrap()
    })
}

/// Redact common token patterns in free text (best-effort; not a full DLP engine).
pub fn redact_secrets(input: &str) -> String {
    let s = pat_bearer().replace_all(input, "${1}[REDACTED]");
    let s = pat_sk().replace_all(&s, "[REDACTED_SK]");
    pat_kv()
        .replace_all(&s, |caps: &regex::Captures| {
            format!("{}{}{}[REDACTED]", &caps[1], &caps[2], &caps[3])
        })
        .into_owned()
}
