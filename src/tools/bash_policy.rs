//! Bash command safety (Claude layer 6–7 parity): strict preflight without a full shell AST.
//!
//! Set `HSM_BASH_POLICY=strict` to reject pipelines, redirections, command substitution, and
//! compound separators **outside** of quoted strings (quote-aware scan).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BashPolicy {
    /// No preflight (default).
    Permissive,
    /// Reject high-risk shell constructs.
    Strict,
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

pub fn bash_policy_from_env() -> BashPolicy {
    match std::env::var("HSM_BASH_POLICY")
        .map(|v| v.trim().to_ascii_lowercase())
        .unwrap_or_default()
        .as_str()
    {
        "strict" | "restricted" | "safe" => BashPolicy::Strict,
        _ if env_truthy("HSM_BASH_STRICT") => BashPolicy::Strict,
        _ => BashPolicy::Permissive,
    }
}

/// Validate `command` for [`BashPolicy::Strict`]. Returns `Ok(())` for permissive.
pub fn validate_bash_command(command: &str, policy: BashPolicy) -> Result<(), String> {
    if policy == BashPolicy::Permissive {
        return Ok(());
    }
    validate_strict_quoted(command)
}

fn validate_strict_quoted(s: &str) -> Result<(), String> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    while i < chars.len() {
        let c = chars[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if in_single {
            if c == '\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            match c {
                '\\' => escaped = true,
                '"' => in_double = false,
                _ => {}
            }
            i += 1;
            continue;
        }

        match c {
            '\'' => in_single = true,
            '"' => in_double = true,
            '\\' => escaped = true,
            '\n' | '\r' => {
                return Err("newlines are not allowed in strict bash policy".into());
            }
            '|' => {
                if i + 1 < chars.len() && chars[i + 1] == '|' {
                    return Err("command lists (||) are not allowed in strict bash policy".into());
                }
                return Err("pipes (|) are not allowed in strict bash policy".into());
            }
            ';' => {
                return Err("command separators (;) are not allowed in strict bash policy".into())
            }
            '`' => {
                return Err(
                    "backtick command substitution is not allowed in strict bash policy".into(),
                )
            }
            '>' | '<' => {
                return Err(
                    "redirections (< > >> <<) are not allowed in strict bash policy".into(),
                );
            }
            '&' => {
                if i + 1 < chars.len() && chars[i + 1] == '&' {
                    return Err("command lists (&&) are not allowed in strict bash policy".into());
                }
                return Err(
                    "background operators (&) are not allowed in strict bash policy".into(),
                );
            }
            '$' if i + 1 < chars.len() => match chars[i + 1] {
                '(' => {
                    return Err(
                        "$(...) command substitution is not allowed in strict bash policy".into(),
                    );
                }
                '{' => {
                    return Err("${...} expansion is not allowed in strict bash policy".into());
                }
                _ => {}
            },
            _ => {}
        }
        i += 1;
    }

    if in_single || in_double {
        return Err("unclosed quote in bash command".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_allows_simple() {
        assert!(validate_bash_command("ls -la", BashPolicy::Strict).is_ok());
        assert!(validate_bash_command("echo 'hello | world'", BashPolicy::Strict).is_ok());
    }

    #[test]
    fn strict_rejects_pipe_outside_quotes() {
        assert!(validate_bash_command("ls | wc", BashPolicy::Strict).is_err());
    }
}
