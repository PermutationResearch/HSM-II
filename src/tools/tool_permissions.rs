//! ECC-style tool firewall: optional exact allowlist + prefix blocklist (env-driven).

use std::collections::HashSet;

/// Permission context checked before every [`super::ToolRegistry::execute`].
#[derive(Clone, Debug)]
pub struct ToolPermissionContext {
    /// If non-empty, only these exact tool names may run.
    allow_exact: Option<HashSet<String>>,
    /// Tool names matching any of these prefixes (after trimming) are denied.
    block_prefixes: Vec<String>,
}

impl ToolPermissionContext {
    /// Read `HSM_TOOL_ALLOW` (comma-separated exact names) and `HSM_TOOL_BLOCK_PREFIXES` (comma-separated prefixes).
    /// When neither is set, all registered tools are allowed (subject only to blocks).
    pub fn from_env() -> Self {
        let allow_exact = std::env::var("HSM_TOOL_ALLOW").ok().and_then(|s| {
            let set: HashSet<String> = s
                .split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect();
            if set.is_empty() {
                None
            } else {
                Some(set)
            }
        });

        let block_prefixes = std::env::var("HSM_TOOL_BLOCK_PREFIXES")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Self {
            allow_exact,
            block_prefixes,
        }
    }

    /// No allowlist restriction, no blocks (tests and safe defaults).
    pub fn permissive() -> Self {
        Self {
            allow_exact: None,
            block_prefixes: Vec::new(),
        }
    }

    /// Allow only these exact tool names.
    pub fn allow_only(names: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        Self {
            allow_exact: Some(names.into_iter().map(|s| s.as_ref().to_string()).collect()),
            block_prefixes: Vec::new(),
        }
    }

    /// Deny tools whose names start with any of these prefixes (tests and programmatic setup).
    pub fn with_blocked_prefixes(prefixes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            allow_exact: None,
            block_prefixes: prefixes.into_iter().map(Into::into).collect(),
        }
    }

    /// Returns `Err(reason)` if the tool must not run.
    pub fn check(&self, tool_name: &str) -> Result<(), String> {
        if let Some(ref allow) = self.allow_exact {
            if !allow.contains(tool_name) {
                return Err(format!(
                    "not in HSM_TOOL_ALLOW allowlist (got '{tool_name}')"
                ));
            }
        }
        for p in &self.block_prefixes {
            if tool_name.starts_with(p) {
                return Err(format!("blocked by prefix '{p}' (HSM_TOOL_BLOCK_PREFIXES)"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_blocks_unknown() {
        let ctx = ToolPermissionContext::allow_only(["read_file", "grep"]);
        assert!(ctx.check("read_file").is_ok());
        assert!(ctx.check("bash").is_err());
    }

    #[test]
    fn block_prefix() {
        let ctx = ToolPermissionContext::with_blocked_prefixes(["git_", "bash"]);
        assert!(ctx.check("read_file").is_ok());
        assert!(ctx.check("git_push").is_err());
        assert!(ctx.check("bash").is_err());
    }
}
