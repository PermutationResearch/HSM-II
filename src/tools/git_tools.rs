//! Git Tools - Version Control Operations

use serde_json::Value;

use super::{Tool, ToolOutput, object_schema};

/// Execute git command and return output
async fn run_git(args: Vec<String>, working_dir: Option<&str>) -> anyhow::Result<(String, String, i32)> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(&args);
    
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    
    let output = cmd.output().await?;
    
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    
    Ok((stdout, stderr, code))
}

// ============================================================================
// Git Status Tool
// ============================================================================

pub struct GitStatusTool;

impl GitStatusTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for GitStatusTool {
    fn name(&self) -> &str {
        "git_status"
    }
    
    fn description(&self) -> &str {
        "Show git working tree status (modified, staged, untracked files)."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("working_dir", "Path to git repository (default: current)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let working_dir = params.get("working_dir").and_then(|v| v.as_str());
        
        match run_git(vec!["status".to_string(), "--short".to_string(), "--branch".to_string()], working_dir).await {
            Ok((stdout, _, 0)) => {
                if stdout.is_empty() {
                    ToolOutput::success("Working tree clean (no changes)".to_string())
                } else {
                    ToolOutput::success(format!("Git status:\n{}", stdout))
                }
            }
            Ok((_, stderr, code)) => {
                ToolOutput::error(format!("Git status failed (exit {}): {}", code, stderr))
            }
            Err(e) => ToolOutput::error(format!("Failed to run git: {}", e)),
        }
    }
}

impl Default for GitStatusTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Git Log Tool
// ============================================================================

pub struct GitLogTool;

impl GitLogTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for GitLogTool {
    fn name(&self) -> &str {
        "git_log"
    }
    
    fn description(&self) -> &str {
        "Show git commit history."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("working_dir", "Path to git repository (default: current)", false),
            ("n", "Number of commits to show (default: 10)", false),
            ("author", "Filter by author (optional)", false),
            ("since", "Show commits since date (e.g., '1 week ago')", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let working_dir = params.get("working_dir").and_then(|v| v.as_str());
        let n = params.get("n").and_then(|v| v.as_u64()).unwrap_or(10);
        
        let mut args = vec!["log".to_string(), "--oneline".to_string(), "--decorate".to_string()];
        args.push(format!("-{}", n));
        
        if let Some(author) = params.get("author").and_then(|v| v.as_str()) {
            args.push("--author".to_string());
            args.push(author.to_string());
        }
        
        if let Some(since) = params.get("since").and_then(|v| v.as_str()) {
            args.push("--since".to_string());
            args.push(since.to_string());
        }
        
        match run_git(args, working_dir).await {
            Ok((stdout, _, 0)) => ToolOutput::success(stdout),
            Ok((_, stderr, code)) => ToolOutput::error(format!("Git log failed (exit {}): {}", code, stderr)),
            Err(e) => ToolOutput::error(format!("Failed to run git: {}", e)),
        }
    }
}

impl Default for GitLogTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Git Diff Tool
// ============================================================================

pub struct GitDiffTool;

impl GitDiffTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for GitDiffTool {
    fn name(&self) -> &str {
        "git_diff"
    }
    
    fn description(&self) -> &str {
        "Show changes between commits, commit and working tree, etc."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("working_dir", "Path to git repository (default: current)", false),
            ("commit", "Commit to compare against (default: HEAD)", false),
            ("staged", "Show staged changes only (default: false)", false),
            ("file", "Show diff for specific file only", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let working_dir = params.get("working_dir").and_then(|v| v.as_str());
        let staged = params.get("staged").and_then(|v| v.as_bool()).unwrap_or(false);
        
        let mut args = vec!["diff".to_string()];
        
        if staged {
            args.push("--staged".to_string());
        }
        
        if let Some(commit) = params.get("commit").and_then(|v| v.as_str()) {
            args.push(commit.to_string());
        }
        
        if let Some(file) = params.get("file").and_then(|v| v.as_str()) {
            args.push("--".to_string());
            args.push(file.to_string());
        }
        
        match run_git(args, working_dir).await {
            Ok((stdout, _, 0)) => {
                if stdout.is_empty() {
                    ToolOutput::success("No differences found".to_string())
                } else {
                    let truncated = if stdout.len() > 10000 {
                        format!("{}...\n[Truncated, total: {} bytes]", &stdout[..10000], stdout.len())
                    } else {
                        stdout
                    };
                    ToolOutput::success(truncated)
                }
            }
            Ok((_, stderr, code)) => ToolOutput::error(format!("Git diff failed (exit {}): {}", code, stderr)),
            Err(e) => ToolOutput::error(format!("Failed to run git: {}", e)),
        }
    }
}

impl Default for GitDiffTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Git Add Tool
// ============================================================================

pub struct GitAddTool;

impl GitAddTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for GitAddTool {
    fn name(&self) -> &str {
        "git_add"
    }
    
    fn description(&self) -> &str {
        "Add file contents to the index (stage files)."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("working_dir", "Path to git repository (default: current)", false),
            ("files", "Files to add (space-separated, or '.' for all)", true),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let working_dir = params.get("working_dir").and_then(|v| v.as_str());
        let files = params.get("files").and_then(|v| v.as_str()).unwrap_or(".");
        
        match run_git(vec!["add".to_string(), files.to_string()], working_dir).await {
            Ok((stdout, _, 0)) => {
                if stdout.is_empty() {
                    ToolOutput::success(format!("Added '{}' to staging area", files))
                } else {
                    ToolOutput::success(stdout)
                }
            }
            Ok((_, stderr, code)) => ToolOutput::error(format!("Git add failed (exit {}): {}", code, stderr)),
            Err(e) => ToolOutput::error(format!("Failed to run git: {}", e)),
        }
    }
}

impl Default for GitAddTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Git Commit Tool
// ============================================================================

pub struct GitCommitTool;

impl GitCommitTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for GitCommitTool {
    fn name(&self) -> &str {
        "git_commit"
    }
    
    fn description(&self) -> &str {
        "Record changes to the repository with a commit message."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("working_dir", "Path to git repository (default: current)", false),
            ("message", "Commit message", true),
            ("no_verify", "Bypass pre-commit hooks (default: false)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let working_dir = params.get("working_dir").and_then(|v| v.as_str());
        let message = params.get("message").and_then(|v| v.as_str()).unwrap_or("");
        
        if message.is_empty() {
            return ToolOutput::error("Commit message is required");
        }
        
        let no_verify = params.get("no_verify").and_then(|v| v.as_bool()).unwrap_or(false);
        
        let mut args = vec!["commit".to_string(), "-m".to_string(), message.to_string()];
        if no_verify {
            args.push("--no-verify".to_string());
        }
        
        match run_git(args, working_dir).await {
            Ok((_, _, 0)) => ToolOutput::success(format!("Committed: {}", message)),
            Ok((_, stderr, code)) => {
                if stderr.contains("nothing to commit") {
                    ToolOutput::success("Nothing to commit, working tree clean".to_string())
                } else {
                    ToolOutput::error(format!("Git commit failed (exit {}): {}", code, stderr))
                }
            }
            Err(e) => ToolOutput::error(format!("Failed to run git: {}", e)),
        }
    }
}

impl Default for GitCommitTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Git Push Tool
// ============================================================================

pub struct GitPushTool;

impl GitPushTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for GitPushTool {
    fn name(&self) -> &str {
        "git_push"
    }
    
    fn description(&self) -> &str {
        "Update remote refs along with associated objects (push commits)."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("working_dir", "Path to git repository (default: current)", false),
            ("remote", "Remote name (default: origin)", false),
            ("branch", "Branch name (default: current branch)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let working_dir = params.get("working_dir").and_then(|v| v.as_str());
        let remote = params.get("remote").and_then(|v| v.as_str()).unwrap_or("origin");
        
        let mut args = vec!["push".to_string(), remote.to_string()];
        
        if let Some(branch) = params.get("branch").and_then(|v| v.as_str()) {
            args.push(branch.to_string());
        }
        
        match run_git(args, working_dir).await {
            Ok((stdout, _, 0)) => {
                if stdout.is_empty() {
                    ToolOutput::success("Push successful".to_string())
                } else {
                    ToolOutput::success(stdout)
                }
            }
            Ok((_, stderr, code)) => ToolOutput::error(format!("Git push failed (exit {}): {}", code, stderr)),
            Err(e) => ToolOutput::error(format!("Failed to run git: {}", e)),
        }
    }
}

impl Default for GitPushTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Git Pull Tool
// ============================================================================

pub struct GitPullTool;

impl GitPullTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for GitPullTool {
    fn name(&self) -> &str {
        "git_pull"
    }
    
    fn description(&self) -> &str {
        "Fetch from and integrate with another repository or local branch."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("working_dir", "Path to git repository (default: current)", false),
            ("remote", "Remote name (default: origin)", false),
            ("branch", "Branch name (default: current branch)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let working_dir = params.get("working_dir").and_then(|v| v.as_str());
        let remote = params.get("remote").and_then(|v| v.as_str()).unwrap_or("origin");
        
        let mut args = vec!["pull".to_string(), remote.to_string()];
        
        if let Some(branch) = params.get("branch").and_then(|v| v.as_str()) {
            args.push(branch.to_string());
        }
        
        match run_git(args, working_dir).await {
            Ok((stdout, _, 0)) => ToolOutput::success(stdout),
            Ok((_, stderr, code)) => ToolOutput::error(format!("Git pull failed (exit {}): {}", code, stderr)),
            Err(e) => ToolOutput::error(format!("Failed to run git: {}", e)),
        }
    }
}

impl Default for GitPullTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Git Branch Tool
// ============================================================================

pub struct GitBranchTool;

impl GitBranchTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for GitBranchTool {
    fn name(&self) -> &str {
        "git_branch"
    }
    
    fn description(&self) -> &str {
        "List, create, or delete branches."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("working_dir", "Path to git repository (default: current)", false),
            ("create", "Name of new branch to create", false),
            ("delete", "Name of branch to delete", false),
            ("list", "List branches (default: true if no other action)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let working_dir = params.get("working_dir").and_then(|v| v.as_str());
        
        let mut args = vec!["branch".to_string()];
        
        if let Some(create) = params.get("create").and_then(|v| v.as_str()) {
            args.push(create.to_string());
        } else if let Some(delete) = params.get("delete").and_then(|v| v.as_str()) {
            args.push("-d".to_string());
            args.push(delete.to_string());
        } else {
            args.push("-vv".to_string()); // Verbose list with upstream
        }
        
        match run_git(args, working_dir).await {
            Ok((stdout, _, 0)) => ToolOutput::success(stdout),
            Ok((_, stderr, code)) => ToolOutput::error(format!("Git branch failed (exit {}): {}", code, stderr)),
            Err(e) => ToolOutput::error(format!("Failed to run git: {}", e)),
        }
    }
}

impl Default for GitBranchTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Git Checkout Tool
// ============================================================================

pub struct GitCheckoutTool;

impl GitCheckoutTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for GitCheckoutTool {
    fn name(&self) -> &str {
        "git_checkout"
    }
    
    fn description(&self) -> &str {
        "Switch branches or restore working tree files."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("working_dir", "Path to git repository (default: current)", false),
            ("branch", "Branch name or commit to checkout", true),
            ("create", "Create new branch (default: false)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let working_dir = params.get("working_dir").and_then(|v| v.as_str());
        let branch = params.get("branch").and_then(|v| v.as_str()).unwrap_or("");
        
        if branch.is_empty() {
            return ToolOutput::error("Branch name is required");
        }
        
        let create = params.get("create").and_then(|v| v.as_bool()).unwrap_or(false);
        
        let mut args = vec!["checkout".to_string()];
        if create {
            args.push("-b".to_string());
        }
        args.push(branch.to_string());
        
        match run_git(args, working_dir).await {
            Ok((_, _, 0)) => {
                let msg = if create {
                    format!("Created and switched to new branch '{}'", branch)
                } else {
                    format!("Switched to '{}'", branch)
                };
                ToolOutput::success(msg)
            }
            Ok((_, stderr, code)) => ToolOutput::error(format!("Git checkout failed (exit {}): {}", code, stderr)),
            Err(e) => ToolOutput::error(format!("Failed to run git: {}", e)),
        }
    }
}

impl Default for GitCheckoutTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Git Clone Tool
// ============================================================================

pub struct GitCloneTool;

impl GitCloneTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for GitCloneTool {
    fn name(&self) -> &str {
        "git_clone"
    }
    
    fn description(&self) -> &str {
        "Clone a repository into a new directory."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("url", "Repository URL to clone", true),
            ("directory", "Directory to clone into (optional)", false),
            ("depth", "Create shallow clone with history truncated to n commits", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let url = params.get("url").and_then(|v| v.as_str()).unwrap_or("");
        
        if url.is_empty() {
            return ToolOutput::error("Repository URL is required");
        }
        
        let mut args = vec!["clone".to_string()];
        
        if let Some(depth) = params.get("depth").and_then(|v| v.as_u64()) {
            args.push("--depth".to_string());
            args.push(depth.to_string());
        }
        
        args.push(url.to_string());
        
        if let Some(dir) = params.get("directory").and_then(|v| v.as_str()) {
            args.push(dir.to_string());
        }
        
        match run_git(args, None).await {
            Ok((_, _, 0)) => {
                let dir = params.get("directory").and_then(|v| v.as_str());
                let msg = dir.map(|d| format!("Cloned into {}", d))
                    .unwrap_or_else(|| "Cloned repository".to_string());
                ToolOutput::success(msg)
            }
            Ok((_, stderr, code)) => ToolOutput::error(format!("Git clone failed (exit {}): {}", code, stderr)),
            Err(e) => ToolOutput::error(format!("Failed to run git: {}", e)),
        }
    }
}

impl Default for GitCloneTool {
    fn default() -> Self {
        Self::new()
    }
}
