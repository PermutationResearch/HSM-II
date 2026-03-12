//! System Tools - System information and utilities

use serde_json::Value;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use super::{Tool, ToolOutput, object_schema};

// ============================================================================
// System Info Tool
// ============================================================================

pub struct SystemInfoTool;

impl SystemInfoTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for SystemInfoTool {
    fn name(&self) -> &str {
        "system_info"
    }
    
    fn description(&self) -> &str {
        "Get system information: OS, architecture, CPU count, memory."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("detail", "brief or full (default: brief)", false),
        ])
    }
    
    async fn execute(&self, _params: Value) -> ToolOutput {
        let info = serde_json::json!({
            "os": std::env::consts::OS,
            "family": std::env::consts::FAMILY,
            "arch": std::env::consts::ARCH,
            "cpus": num_cpus::get(),
            "physical_cpus": num_cpus::get_physical(),
            "cwd": std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string()),
            "home": std::env::var("HOME").ok().or_else(|| std::env::var("USERPROFILE").ok()),
            "temp_dir": std::env::temp_dir().to_string_lossy().to_string(),
        });
        
        let summary = format!(
            "OS: {} {}, {} CPUs",
            info["os"].as_str().unwrap_or("unknown"),
            info["arch"].as_str().unwrap_or("unknown"),
            info["cpus"].as_u64().unwrap_or(0)
        );
        
        ToolOutput::success(summary)
            .with_metadata(info)
    }
}

impl Default for SystemInfoTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Environment Tool
// ============================================================================

pub struct EnvironmentTool;

impl EnvironmentTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for EnvironmentTool {
    fn name(&self) -> &str {
        "env"
    }
    
    fn description(&self) -> &str {
        "Get or set environment variables."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("get", "Name of environment variable to get", false),
            ("set", "JSON object of variables to set", false),
            ("list", "List all environment variables (default: false)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        if let Some(var_name) = params.get("get").and_then(|v| v.as_str()) {
            match std::env::var(var_name) {
                Ok(value) => ToolOutput::success(value),
                Err(_) => ToolOutput::error(format!("Environment variable '{}' not found", var_name)),
            }
        } else if let Some(vars) = params.get("set").and_then(|v| v.as_object()) {
            for (key, value) in vars {
                if let Some(val_str) = value.as_str() {
                    std::env::set_var(key, val_str);
                }
            }
            ToolOutput::success(format!("Set {} environment variables", vars.len()))
        } else if params.get("list").and_then(|v| v.as_bool()).unwrap_or(false) {
            let vars: std::collections::HashMap<String, String> = std::env::vars().collect();
            let list: Vec<String> = vars
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            ToolOutput::success(list.join("\n"))
        } else {
            ToolOutput::error("Specify 'get', 'set', or 'list'")
        }
    }
}

impl Default for EnvironmentTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Process List Tool
// ============================================================================

pub struct ProcessListTool;

impl ProcessListTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for ProcessListTool {
    fn name(&self) -> &str {
        "process_list"
    }
    
    fn description(&self) -> &str {
        "List running processes (limited info on non-Linux systems)."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("filter", "Filter by process name (optional)", false),
            ("limit", "Maximum number of processes to return (default: 50)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let filter = params.get("filter").and_then(|v| v.as_str());
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
        
        // Try to get process list using ps command
        let output = tokio::process::Command::new("ps")
            .args(&["aux"])
            .output()
            .await;
        
        match output {
            Ok(result) if result.status.success() => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                let lines: Vec<&str> = stdout.lines().collect();
                
                let mut filtered: Vec<&str> = lines.iter()
                    .filter(|line| {
                        if let Some(f) = filter {
                            line.to_lowercase().contains(&f.to_lowercase())
                        } else {
                            true
                        }
                    })
                    .copied()
                    .take(limit)
                    .collect();
                
                // Keep header
                if !lines.is_empty() && filtered.len() > 1 {
                    filtered.insert(0, lines[0]);
                }
                
                ToolOutput::success(filtered.join("\n"))
            }
            _ => {
                // Fallback: just show this process info
                let pid = std::process::id();
                ToolOutput::success(format!("Current process PID: {}\n(Full process list unavailable on this system)", pid))
            }
        }
    }
}

impl Default for ProcessListTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Disk Usage Tool
// ============================================================================

pub struct DiskUsageTool;

impl DiskUsageTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for DiskUsageTool {
    fn name(&self) -> &str {
        "disk_usage"
    }
    
    fn description(&self) -> &str {
        "Show disk usage for a directory or filesystem."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("path", "Path to check (default: current directory)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let path = params.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        
        // Try df for filesystem info
        let output = tokio::process::Command::new("df")
            .args(&["-h", path])
            .output()
            .await;
        
        match output {
            Ok(result) if result.status.success() => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                ToolOutput::success(stdout.to_string())
            }
            _ => {
                // Fallback: try du
                let output = tokio::process::Command::new("du")
                    .args(&["-sh", path])
                    .output()
                    .await;
                
                match output {
                    Ok(result) if result.status.success() => {
                        let stdout = String::from_utf8_lossy(&result.stdout);
                        ToolOutput::success(format!("Directory size: {}", stdout.trim()))
                    }
                    _ => ToolOutput::error("Could not get disk usage"),
                }
            }
        }
    }
}

impl Default for DiskUsageTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// File Info Tool
// ============================================================================

pub struct FileInfoTool;

impl FileInfoTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for FileInfoTool {
    fn name(&self) -> &str {
        "file_info"
    }
    
    fn description(&self) -> &str {
        "Get detailed information about a file: size, type, permissions, modified time."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("path", "Path to file", true),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
        
        if path.is_empty() {
            return ToolOutput::error("path is required");
        }
        
        let path_obj = std::path::Path::new(path);
        
        match tokio::fs::metadata(path).await {
            Ok(metadata) => {
                let permissions = {
                    let mode = metadata.permissions().mode();
                    format!("{:o}", mode & 0o7777)
                };
                
                let info = serde_json::json!({
                    "exists": true,
                    "is_file": metadata.is_file(),
                    "is_dir": metadata.is_dir(),
                    "is_symlink": metadata.is_symlink(),
                    "size": metadata.len(),
                    "size_human": Self::human_readable_size(metadata.len()),
                    "modified": metadata.modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs()),
                    "accessed": metadata.accessed()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs()),
                    "created": metadata.created()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs()),
                    "permissions": permissions,
                    "extension": path_obj.extension()
                        .and_then(|e| e.to_str()),
                });
                
                let summary = if metadata.is_dir() {
                    format!("Directory: {} ({} bytes)", path, metadata.len())
                } else {
                    format!("File: {} ({})", path, Self::human_readable_size(metadata.len()))
                };
                
                ToolOutput::success(summary)
                    .with_metadata(info)
            }
            Err(e) => ToolOutput::error(format!("Cannot get file info: {}", e)),
        }
    }
}

impl FileInfoTool {
    fn human_readable_size(bytes: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
        let mut size = bytes as f64;
        let mut unit_idx = 0;
        
        while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
            size /= 1024.0;
            unit_idx += 1;
        }
        
        format!("{:.2} {}", size, UNITS[unit_idx])
    }
}

impl Default for FileInfoTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// List Directory Tool (Enhanced)
// ============================================================================

pub struct ListDirectoryTool;

impl ListDirectoryTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for ListDirectoryTool {
    fn name(&self) -> &str {
        "list_directory"
    }
    
    fn description(&self) -> &str {
        "List directory contents with optional filtering and sorting."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("path", "Directory path (default: current)", false),
            ("pattern", "Glob pattern to filter (e.g., '*.rs')", false),
            ("recursive", "List recursively (default: false)", false),
            ("sort", "Sort by: name, size, time (default: name)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let path = params.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let pattern = params.get("pattern").and_then(|v| v.as_str());
        let recursive = params.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false);
        let sort = params.get("sort").and_then(|v| v.as_str()).unwrap_or("name");
        
        let path_obj = std::path::PathBuf::from(path);
        
        if recursive {
            // Use glob for recursive listing
            let pattern_str = pattern.unwrap_or("*");
            let full_pattern = format!("{}/**/{}", path, pattern_str);
            
            let mut entries = Vec::new();
            
            match glob::glob(&full_pattern) {
                Ok(paths) => {
                    for entry in paths.flatten() {
                        if entry.file_name().is_some() {
                            entries.push(format!(
                                "{} {}",
                                if entry.is_dir() { "[D]" } else { "[F]" },
                                entry.strip_prefix(&path_obj).unwrap_or(&entry).display()
                            ));
                        }
                    }
                }
                Err(e) => return ToolOutput::error(format!("Glob error: {}", e)),
            }
            
            entries.sort();
            ToolOutput::success(entries.join("\n"))
        } else {
            // Simple directory listing
            match tokio::fs::read_dir(&path_obj).await {
                Ok(mut entries) => {
                    struct DirItem {
                        name: String,
                        size: u64,
                        modified: std::time::SystemTime,
                        file_type: String,
                    }
                    
                    let mut items: Vec<DirItem> = Vec::new();
                    
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let name = entry.file_name().to_string_lossy().to_string();
                        
                        // Filter by pattern
                        if let Some(pat) = pattern {
                            if !glob::Pattern::new(pat)
                                .map(|p| p.matches(&name))
                                .unwrap_or(true) {
                                continue;
                            }
                        }
                        
                        let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
                        let file_type = if is_dir { "[D]".to_string() } else { "[F]".to_string() };
                        
                        let (size, modified) = if let Ok(metadata) = entry.metadata().await {
                            let mod_time = metadata.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            (metadata.len(), mod_time)
                        } else {
                            (0, std::time::SystemTime::UNIX_EPOCH)
                        };
                        
                        items.push(DirItem { name, size, modified, file_type });
                    }
                    
                    // Sort
                    match sort {
                        "size" => items.sort_by(|a, b| b.size.cmp(&a.size)),
                        "time" => items.sort_by(|a, b| b.modified.cmp(&a.modified)),
                        _ => items.sort_by(|a, b| a.name.cmp(&b.name)),
                    }
                    
                    let output: Vec<String> = items
                        .into_iter()
                        .map(|item| {
                            format!("{} {:>10} {}", item.file_type, item.size, item.name)
                        })
                        .collect();
                    
                    ToolOutput::success(output.join("\n"))
                }
                Err(e) => ToolOutput::error(format!("Cannot read directory: {}", e)),
            }
        }
    }
}

impl Default for ListDirectoryTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Read File Tool (Enhanced with offset/lines)
// ============================================================================

pub struct ReadFileEnhancedTool;

impl ReadFileEnhancedTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for ReadFileEnhancedTool {
    fn name(&self) -> &str {
        "read_file"
    }
    
    fn description(&self) -> &str {
        "Read file contents with optional offset and line limit."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("path", "File path to read", true),
            ("offset", "Line offset to start from (0-based, default: 0)", false),
            ("limit", "Maximum number of lines to read (default: 1000)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let offset = params.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(1000) as usize;
        
        if path.is_empty() {
            return ToolOutput::error("path is required");
        }
        
        match tokio::fs::read_to_string(path).await {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let start = offset.min(lines.len());
                let end = (start + limit).min(lines.len());
                
                let selected: Vec<&str> = lines[start..end].to_vec();
                let output = selected.join("\n");
                
                let status = if end < lines.len() {
                    format!("Lines {}-{} of {}", start + 1, end, lines.len())
                } else {
                    format!("Lines {}-{}", start + 1, end)
                };
                
                ToolOutput::success(format!("{}\n\n[{}]", output, status))
                    .with_metadata(serde_json::json!({
                        "total_lines": lines.len(),
                        "start_line": start + 1,
                        "end_line": end,
                    }))
            }
            Err(e) => ToolOutput::error(format!("Failed to read file: {}", e)),
        }
    }
}

impl Default for ReadFileEnhancedTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Search Files Tool (find + grep combined)
// ============================================================================

pub struct SearchFilesTool;

impl SearchFilesTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }
    
    fn description(&self) -> &str {
        "Search for files and optionally search within file contents."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("path", "Directory to search (default: current)", false),
            ("name_pattern", "File name pattern (e.g., '*.rs')", false),
            ("content_pattern", "Text pattern to search in file contents", false),
            ("case_sensitive", "Case-sensitive content search (default: false)", false),
            ("max_results", "Maximum results (default: 100)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let path = params.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let name_pattern = params.get("name_pattern").and_then(|v| v.as_str());
        let content_pattern = params.get("content_pattern").and_then(|v| v.as_str());
        let case_sensitive = params.get("case_sensitive").and_then(|v| v.as_bool()).unwrap_or(false);
        let max_results = params.get("max_results").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
        
        let mut results = Vec::new();
        
        // Find files
        let pattern = format!("{}/{}", path, name_pattern.unwrap_or("*"));
        
        match glob::glob(&pattern) {
            Ok(paths) => {
                for entry in paths.flatten() {
                    if results.len() >= max_results {
                        break;
                    }
                    
                    if entry.is_file() {
                        let mut matches = true;
                        
                        // Check content if pattern provided
                        if let Some(content_pat) = content_pattern {
                            if let Ok(content) = tokio::fs::read_to_string(&entry).await {
                                let content_check = if case_sensitive {
                                    content.contains(content_pat)
                                } else {
                                    content.to_lowercase().contains(&content_pat.to_lowercase())
                                };
                                matches = content_check;
                                
                                if matches {
                                    // Find matching lines
                                    let lines: Vec<String> = content
                                        .lines()
                                        .enumerate()
                                        .filter(|(_, line)| {
                                            if case_sensitive {
                                                line.contains(content_pat)
                                            } else {
                                                line.to_lowercase().contains(&content_pat.to_lowercase())
                                            }
                                        })
                                        .map(|(i, line)| format!("  Line {}: {}", i + 1, line.trim()))
                                        .take(3)
                                        .collect();
                                    
                                    results.push(format!("{}:\n{}", entry.display(), lines.join("\n")));
                                    continue;
                                }
                            }
                        }
                        
                        if matches && content_pattern.is_none() {
                            results.push(entry.display().to_string());
                        }
                    }
                }
            }
            Err(e) => return ToolOutput::error(format!("Pattern error: {}", e)),
        }
        
        if results.is_empty() {
            ToolOutput::success("No matching files found".to_string())
        } else {
            ToolOutput::success(format!("Found {} files:\n{}", results.len(), results.join("\n")))
        }
    }
}

impl Default for SearchFilesTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Archive Extract Tool
// ============================================================================

pub struct ArchiveExtractTool;

impl ArchiveExtractTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for ArchiveExtractTool {
    fn name(&self) -> &str {
        "archive_extract"
    }
    
    fn description(&self) -> &str {
        "Extract zip, tar, tar.gz archives."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("archive", "Path to archive file", true),
            ("destination", "Destination directory (default: current)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let archive = params.get("archive").and_then(|v| v.as_str()).unwrap_or("");
        let destination = params.get("destination").and_then(|v| v.as_str()).unwrap_or(".");
        
        if archive.is_empty() {
            return ToolOutput::error("archive path is required");
        }
        
        let archive_path = std::path::Path::new(archive);
        
        if !archive_path.exists() {
            return ToolOutput::error(format!("Archive not found: {}", archive));
        }
        
        let extension = archive_path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        
        let result = match extension.as_str() {
            "zip" => {
                tokio::process::Command::new("unzip")
                    .args(&["-o", archive, "-d", destination])
                    .output()
                    .await
            }
            "gz" if archive.ends_with(".tar.gz") || archive.ends_with(".tgz") => {
                tokio::process::Command::new("tar")
                    .args(&["-xzf", archive, "-C", destination])
                    .output()
                    .await
            }
            "tar" => {
                tokio::process::Command::new("tar")
                    .args(&["-xf", archive, "-C", destination])
                    .output()
                    .await
            }
            "bz2" => {
                tokio::process::Command::new("tar")
                    .args(&["-xjf", archive, "-C", destination])
                    .output()
                    .await
            }
            _ => {
                return ToolOutput::error(format!("Unsupported archive format: {}", extension));
            }
        };
        
        match result {
            Ok(output) if output.status.success() => {
                ToolOutput::success(format!("Extracted '{}' to '{}'", archive, destination))
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                ToolOutput::error(format!("Extraction failed: {}", stderr))
            }
            Err(e) => ToolOutput::error(format!("Failed to run extractor: {}", e)),
        }
    }
}

impl Default for ArchiveExtractTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Archive Create Tool
// ============================================================================

pub struct ArchiveCreateTool;

impl ArchiveCreateTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for ArchiveCreateTool {
    fn name(&self) -> &str {
        "archive_create"
    }
    
    fn description(&self) -> &str {
        "Create zip or tar archives."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("output", "Output archive path", true),
            ("files", "Files/directories to include (space-separated or array)", true),
            ("format", "Archive format: zip, tar, tar.gz (auto-detected from extension if not specified)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let output = params.get("output").and_then(|v| v.as_str()).unwrap_or("");
        
        if output.is_empty() {
            return ToolOutput::error("output path is required");
        }
        
        let files = if let Some(arr) = params.get("files").and_then(|v| v.as_array()) {
            arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect::<Vec<_>>()
        } else if let Some(s) = params.get("files").and_then(|v| v.as_str()) {
            s.split_whitespace().map(|s| s.to_string()).collect::<Vec<_>>()
        } else {
            return ToolOutput::error("files parameter is required");
        };
        
        if files.is_empty() {
            return ToolOutput::error("No files specified");
        }
        
        let format = params.get("format").and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                if output.ends_with(".zip") {
                    "zip".to_string()
                } else if output.ends_with(".tar.gz") || output.ends_with(".tgz") {
                    "tar.gz".to_string()
                } else {
                    "tar".to_string()
                }
            });
        
        let result = match format.as_str() {
            "zip" => {
                let mut args = vec!["-r", output];
                args.extend(files.iter().map(|s| s.as_str()));
                tokio::process::Command::new("zip")
                    .args(&args)
                    .output()
                    .await
            }
            "tar" => {
                let mut args = vec!["-cf", output];
                args.extend(files.iter().map(|s| s.as_str()));
                tokio::process::Command::new("tar")
                    .args(&args)
                    .output()
                    .await
            }
            "tar.gz" => {
                let mut args = vec!["-czf", output];
                args.extend(files.iter().map(|s| s.as_str()));
                tokio::process::Command::new("tar")
                    .args(&args)
                    .output()
                    .await
            }
            _ => return ToolOutput::error(format!("Unsupported format: {}", format)),
        };
        
        match result {
            Ok(result) if result.status.success() => {
                ToolOutput::success(format!("Created archive: {}", output))
            }
            Ok(result) => {
                let stderr = String::from_utf8_lossy(&result.stderr);
                ToolOutput::error(format!("Archive creation failed: {}", stderr))
            }
            Err(e) => ToolOutput::error(format!("Failed to create archive: {}", e)),
        }
    }
}

impl Default for ArchiveCreateTool {
    fn default() -> Self {
        Self::new()
    }
}
