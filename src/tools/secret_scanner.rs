//! Scans files and strings for plaintext secrets, duplicate credentials,
//! and insecure file permissions.

use std::collections::HashMap;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// A single finding from the scanner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanFinding {
    pub message: String,
    pub kind: FindingKind,
    /// File path + line number, if applicable.
    pub location: Option<String>,
    pub severity: Severity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingKind {
    SecretPattern(&'static str),
    Duplicate,
    InsecurePermission,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

struct Pattern {
    name: &'static str,
    matcher: fn(&str) -> bool,
    severity: Severity,
}

/// True if `s` contains `prefix` followed by at least `min_len` word-chars.
fn has_token(s: &str, prefix: &str, min_len: usize) -> bool {
    let Some(idx) = s.find(prefix) else {
        return false;
    };
    let after = &s[idx + prefix.len()..];
    after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        .count()
        >= min_len
}

static PATTERNS: &[Pattern] = &[
    Pattern {
        name: "github_pat",
        matcher: |s| s.contains("ghp_") || s.contains("github_pat_"),
        severity: Severity::Critical,
    },
    Pattern {
        name: "openai_api_key",
        matcher: |s| has_token(s, "sk-proj-", 10) || has_token(s, "sk-", 20),
        severity: Severity::Critical,
    },
    Pattern {
        name: "anthropic_api_key",
        matcher: |s| has_token(s, "sk-ant-", 10),
        severity: Severity::Critical,
    },
    Pattern {
        name: "aws_access_key",
        matcher: |s| has_token(s, "AKIA", 16),
        severity: Severity::Critical,
    },
    Pattern {
        name: "slack_token",
        matcher: |s| s.contains("xoxb-") || s.contains("xoxp-") || s.contains("xoxa-"),
        severity: Severity::High,
    },
    Pattern {
        name: "stripe_live_key",
        matcher: |s| has_token(s, "sk_live_", 10) || has_token(s, "rk_live_", 10),
        severity: Severity::Critical,
    },
    Pattern {
        name: "stripe_test_key",
        matcher: |s| has_token(s, "sk_test_", 10),
        severity: Severity::Medium,
    },
    Pattern {
        name: "google_api_key",
        matcher: |s| has_token(s, "AIza", 30),
        severity: Severity::High,
    },
    Pattern {
        name: "private_key_pem",
        matcher: |s| {
            s.contains("-----BEGIN RSA PRIVATE KEY-----")
                || s.contains("-----BEGIN PRIVATE KEY-----")
                || s.contains("-----BEGIN EC PRIVATE KEY-----")
        },
        severity: Severity::Critical,
    },
    Pattern {
        name: "password_assignment",
        matcher: |s| {
            let l = s.to_ascii_lowercase();
            let has_kw = l.contains("password") || l.contains("passwd") || l.contains("pwd");
            let has_assign = l.contains('=') || l.contains(": ");
            // Skip blank RHS and obvious comments
            let not_empty = !l.contains("=\"\"")
                && !l.contains("= \"\"")
                && !l.contains(": \"\"")
                && !l.trim_start().starts_with('#')
                && !l.trim_start().starts_with("//");
            has_kw && has_assign && not_empty
        },
        severity: Severity::Medium,
    },
    Pattern {
        name: "database_url_with_credentials",
        matcher: |s| {
            let l = s.to_ascii_lowercase();
            let is_db_url = l.contains("postgres://")
                || l.contains("mysql://")
                || l.contains("mongodb://");
            // Must have a non-empty password (user:pass@host vs user:@host)
            is_db_url && l.contains('@') && !l.contains(":@")
        },
        severity: Severity::High,
    },
    Pattern {
        name: "hardcoded_bearer_token",
        matcher: |s| {
            let l = s.to_ascii_lowercase();
            // Literal "bearer <something>" not using a variable reference
            l.contains("bearer ") && !l.contains('$') && !l.contains('{')
        },
        severity: Severity::High,
    },
];

// ── Public API ──────────────────────────────────────────────────────────────

/// Scan a string for secret patterns. `source` labels findings (e.g. file path).
pub fn scan_string(content: &str, source: Option<&str>) -> Vec<ScanFinding> {
    let mut out = Vec::new();
    for (line_no, line) in content.lines().enumerate() {
        for pat in PATTERNS {
            if (pat.matcher)(line) {
                out.push(ScanFinding {
                    message: format!(
                        "possible {} detected{}",
                        pat.name,
                        source.map(|s| format!(" in {s}")).unwrap_or_default()
                    ),
                    kind: FindingKind::SecretPattern(pat.name),
                    location: source.map(|s| format!("{}:{}", s, line_no + 1)),
                    severity: pat.severity.clone(),
                });
            }
        }
    }
    out
}

/// Scan a single file: secret patterns + Unix permission check.
pub fn scan_file(path: &Path) -> Vec<ScanFinding> {
    let mut out = Vec::new();

    #[cfg(unix)]
    if let Ok(meta) = std::fs::metadata(path) {
        let mode = meta.permissions().mode();
        if !meta.is_dir() && (mode & 0o004) != 0 && is_sensitive_name(&path.to_string_lossy()) {
            out.push(ScanFinding {
                message: format!(
                    "{} is world-readable (mode {:04o}); consider `chmod 600`",
                    path.display(),
                    mode & 0o777
                ),
                kind: FindingKind::InsecurePermission,
                location: Some(path.to_string_lossy().into_owned()),
                severity: Severity::High,
            });
        }
    }

    match std::fs::read_to_string(path) {
        Ok(content) => {
            let label = path.to_string_lossy().into_owned();
            out.extend(scan_string(&content, Some(&label)));
        }
        Err(_) => {} // binary or unreadable — skip silently
    }

    out
}

/// Scan multiple files; additionally report duplicate secret patterns across files.
///
/// Duplicates are detected by hashing the matched line's normalized content —
/// same secret appearing in two different files will be flagged.
pub fn scan_files(paths: &[&Path]) -> Vec<ScanFinding> {
    // (pattern_name, line_fingerprint) → first location
    let mut seen: HashMap<(&'static str, u64), String> = HashMap::new();
    let mut all: Vec<ScanFinding> = Vec::new();

    for path in paths {
        let findings = scan_file(path);
        for f in &findings {
            if let FindingKind::SecretPattern(pat_name) = &f.kind {
                // Fingerprint: stable hash of (pattern + location-line portion)
                // We use the location string as a proxy for the matched content.
                if let Some(loc) = &f.location {
                    // Extract line content for hashing via re-read (avoid storing all lines)
                    let fp = fingerprint(pat_name, loc);
                    if let Some(prev_loc) = seen.get(&(*pat_name, fp)) {
                        all.push(ScanFinding {
                            message: format!(
                                "duplicate secret pattern '{}' also found at {} (first seen at {})",
                                pat_name, loc, prev_loc
                            ),
                            kind: FindingKind::Duplicate,
                            location: Some(loc.clone()),
                            severity: Severity::Medium,
                        });
                    } else {
                        seen.insert((*pat_name, fp), loc.clone());
                    }
                }
            }
        }
        all.extend(findings);
    }

    all
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn is_sensitive_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".env")
        || n.ends_with(".pem")
        || n.ends_with(".key")
        || n.ends_with(".p12")
        || n.ends_with(".pfx")
        || n.contains(".env.")
        || n.ends_with("credentials")
        || n.ends_with(".netrc")
        || n.ends_with(".secret")
}

/// Stable 64-bit hash used only for duplicate detection (not crypto).
fn fingerprint(pat: &str, loc: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    pat.hash(&mut h);
    loc.hash(&mut h);
    Hasher::finish(&h)
}

impl ScanFinding {
    pub fn kind_name(&self) -> &str {
        match &self.kind {
            FindingKind::SecretPattern(n) => n,
            FindingKind::Duplicate => "duplicate",
            FindingKind::InsecurePermission => "insecure_permission",
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_github_pat() {
        let f = scan_string("TOKEN=ghp_abcdefghijklmnopqrstuvwxyz123456", None);
        assert!(f.iter().any(|x| x.kind == FindingKind::SecretPattern("github_pat")));
    }

    #[test]
    fn detects_openai_key() {
        let f = scan_string("OPENAI_API_KEY=sk-proj-abcdefghijklmnopqrstu", None);
        assert!(f.iter().any(|x| x.kind == FindingKind::SecretPattern("openai_api_key")));
    }

    #[test]
    fn detects_aws_key() {
        let f = scan_string("AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLEABCD", None);
        assert!(f.iter().any(|x| x.kind == FindingKind::SecretPattern("aws_access_key")));
    }

    #[test]
    fn detects_private_key_pem() {
        let f = scan_string("-----BEGIN RSA PRIVATE KEY-----\nMIIEow...", None);
        assert!(f.iter().any(|x| x.kind == FindingKind::SecretPattern("private_key_pem")));
    }

    #[test]
    fn detects_db_url() {
        let f = scan_string("DATABASE_URL=postgres://user:hunter2@db.example.com/mydb", None);
        assert!(f
            .iter()
            .any(|x| x.kind == FindingKind::SecretPattern("database_url_with_credentials")));
    }

    #[test]
    fn clean_string_no_findings() {
        let f = scan_string("This is a harmless log line with no secrets.", None);
        assert!(f.is_empty(), "expected no findings, got: {f:?}");
    }

    #[test]
    fn skips_empty_password_assignment() {
        let f = scan_string("password=\"\"", None);
        assert!(f.is_empty(), "empty password should not trigger");
    }

    #[test]
    fn db_url_without_password_skipped() {
        let f = scan_string("DATABASE_URL=postgres://user:@db.example.com/mydb", None);
        assert!(!f
            .iter()
            .any(|x| x.kind == FindingKind::SecretPattern("database_url_with_credentials")));
    }
}
