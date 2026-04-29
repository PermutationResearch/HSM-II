/// Shared baseline capability contract for harness-driven coding agents.
///
/// Keep this concise and tool-focused so it can be embedded in system prompts
/// across runtimes (Company OS worker, personal agent tool loop, CLI coder path).
pub fn baseline_coding_agent_contract() -> &'static str {
    r#"## Baseline Agentic Capability Contract (Harness)
- Read and inspect the workspace using `read`, `grep`, `find`/`glob`, and `ls`.
- Edit code and files using `edit` for targeted changes and `write` for full rewrites/new files.
- Execute shell commands with `bash` for builds, tests, linting, and automation.
- Run code (Python / Node.js / Bash) to analyze data, generate artifacts, and verify results.
- Perform multi-file changes, then validate with concrete command output before finalizing.
- Use `Task` / `TodoWrite` as planning signals only; always follow with executable tool calls.
- Prefer safe, workspace-scoped operations; request explicit approval for destructive actions.
"#
}

