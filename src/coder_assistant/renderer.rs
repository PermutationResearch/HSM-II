//! Differential Rendering for Flicker-Free Updates
//!
//! Provides efficient rendering with synchronized output

use std::collections::VecDeque;

/// Render update types
#[derive(Clone, Debug)]
pub enum RenderUpdate {
    /// Append text at the end
    Append(String),
    /// Replace entire content
    Replace(String),
    /// Update specific line
    UpdateLine { line: usize, content: String },
    /// Insert line at position
    InsertLine { line: usize, content: String },
    /// Delete line at position
    DeleteLine { line: usize },
    /// Clear all content
    Clear,
}

/// Differential renderer for efficient updates
pub struct DifferentialRenderer {
    content: Vec<String>,
    buffer: VecDeque<RenderUpdate>,
    max_buffer_size: usize,
    last_render: String,
}

impl DifferentialRenderer {
    pub fn new() -> Self {
        Self {
            content: Vec::new(),
            buffer: VecDeque::new(),
            max_buffer_size: 100,
            last_render: String::new(),
        }
    }

    /// Queue an update
    pub fn queue(&mut self, update: RenderUpdate) {
        if self.buffer.len() >= self.max_buffer_size {
            // Flush buffer if too large
            self.flush();
        }
        self.buffer.push_back(update);
    }

    /// Append text (streaming)
    pub fn append(&mut self, text: &str) {
        self.queue(RenderUpdate::Append(text.to_string()));
    }

    /// Replace all content
    pub fn replace(&mut self, text: &str) {
        self.queue(RenderUpdate::Replace(text.to_string()));
    }

    /// Update a specific line
    pub fn update_line(&mut self, line: usize, content: &str) {
        self.queue(RenderUpdate::UpdateLine {
            line,
            content: content.to_string(),
        });
    }

    /// Clear all content
    pub fn clear(&mut self) {
        self.queue(RenderUpdate::Clear);
    }

    /// Flush all pending updates and render
    pub fn flush(&mut self) -> String {
        let mut output = String::new();

        while let Some(update) = self.buffer.pop_front() {
            match update {
                RenderUpdate::Append(text) => {
                    if self.content.is_empty() {
                        self.content.push(text);
                    } else {
                        let last = self.content.len() - 1;
                        self.content[last].push_str(&text);
                    }
                }
                RenderUpdate::Replace(text) => {
                    self.content = text.lines().map(|s| s.to_string()).collect();
                }
                RenderUpdate::UpdateLine { line, content } => {
                    if line < self.content.len() {
                        self.content[line] = content;
                    } else {
                        // Pad with empty lines
                        while self.content.len() <= line {
                            self.content.push(String::new());
                        }
                        self.content[line] = content;
                    }
                }
                RenderUpdate::InsertLine { line, content } => {
                    if line <= self.content.len() {
                        self.content.insert(line, content);
                    }
                }
                RenderUpdate::DeleteLine { line } => {
                    if line < self.content.len() {
                        self.content.remove(line);
                    }
                }
                RenderUpdate::Clear => {
                    self.content.clear();
                }
            }
        }

        // Build output
        for (i, line) in self.content.iter().enumerate() {
            if i > 0 {
                output.push('\n');
            }
            output.push_str(line);
        }

        self.last_render = output.clone();
        output
    }

    /// Get current content as string
    pub fn content(&self) -> String {
        self.content.join("\n")
    }

    /// Get content as lines
    pub fn lines(&self) -> &[String] {
        &self.content
    }

    /// Calculate diff between current and new content
    pub fn diff(&self, new_content: &str) -> Vec<RenderUpdate> {
        let new_lines: Vec<&str> = new_content.lines().collect();
        let mut updates = Vec::new();

        // Simple diff: find first changed line
        let mut first_diff = None;
        for (i, (old, new)) in self.content.iter().zip(new_lines.iter()).enumerate() {
            if old != *new {
                first_diff = Some(i);
                break;
            }
        }

        if first_diff.is_none() && self.content.len() != new_lines.len() {
            first_diff = Some(std::cmp::min(self.content.len(), new_lines.len()));
        }

        if let Some(start) = first_diff {
            // Replace from start to end
            for i in start..new_lines.len() {
                if i < self.content.len() {
                    updates.push(RenderUpdate::UpdateLine {
                        line: i,
                        content: new_lines[i].to_string(),
                    });
                } else {
                    updates.push(RenderUpdate::InsertLine {
                        line: i,
                        content: new_lines[i].to_string(),
                    });
                }
            }

            // Delete extra lines
            for i in (new_lines.len()..self.content.len()).rev() {
                updates.push(RenderUpdate::DeleteLine { line: i });
            }
        }

        updates
    }

    /// Smart update - only queue necessary changes
    pub fn smart_update(&mut self, new_content: &str) {
        let updates = self.diff(new_content);
        if updates.is_empty() {
            // No changes needed
            return;
        }

        for update in updates {
            self.queue(update);
        }
    }
}

impl Default for DifferentialRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Synchronized output handler
pub struct SynchronizedOutput {
    _renderer: DifferentialRenderer,
    lock: std::sync::Mutex<()>,
}

impl SynchronizedOutput {
    pub fn new() -> Self {
        Self {
            _renderer: DifferentialRenderer::new(),
            lock: std::sync::Mutex::new(()),
        }
    }

    /// Write output (thread-safe)
    pub fn write(&self, text: &str) {
        let _guard = self.lock.lock().unwrap();
        // In real implementation, this would write to terminal
        print!("{}", text);
    }

    /// Write line (thread-safe)
    pub fn writeln(&self, text: &str) {
        let _guard = self.lock.lock().unwrap();
        println!("{}", text);
    }

    /// Flush output
    pub fn flush(&self) {
        let _guard = self.lock.lock().unwrap();
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }
}

impl Default for SynchronizedOutput {
    fn default() -> Self {
        Self::new()
    }
}

/// Markdown renderer with syntax highlighting hints
pub struct MarkdownRenderer;

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self
    }

    /// Render markdown to terminal-formatted text
    pub fn render(&self, markdown: &str) -> String {
        let mut output = String::new();
        let mut in_code_block = false;
        let mut code_language = String::new();

        for line in markdown.lines() {
            if line.starts_with("```") {
                if in_code_block {
                    // End code block
                    output.push_str("```\n");
                    in_code_block = false;
                    code_language.clear();
                } else {
                    // Start code block
                    code_language = line[3..].trim().to_string();
                    output.push_str(&format!("```{}\n", code_language));
                    in_code_block = true;
                }
            } else if line.starts_with("# ") {
                output.push_str(&format!("\n{}}}", &line[2..]));
            } else if line.starts_with("## ") {
                output.push_str(&format!("\n{}", &line[3..]));
            } else if line.starts_with("### ") {
                output.push_str(&format!("\n{}", &line[4..]));
            } else if line.starts_with("- ") || line.starts_with("* ") {
                output.push_str(&format!("  • {}\n", &line[2..]));
            } else if line.starts_with("> ") {
                output.push_str(&format!("> {}\n", &line[2..]));
            } else {
                output.push_str(line);
                output.push('\n');
            }
        }

        output
    }

    /// Strip markdown formatting
    pub fn strip(markdown: &str) -> String {
        let mut output = String::new();
        let mut in_code_block = false;

        for line in markdown.lines() {
            if line.starts_with("```") {
                in_code_block = !in_code_block;
                continue;
            }

            if in_code_block {
                output.push_str(line);
                output.push('\n');
                continue;
            }

            // Remove inline formatting
            let line = line.replace("**", "").replace("*", "").replace("`", "");

            if line.starts_with("# ") {
                output.push_str(&line[2..]);
            } else if line.starts_with("## ") {
                output.push_str(&line[3..]);
            } else if line.starts_with("### ") {
                output.push_str(&line[4..]);
            } else if line.starts_with("- ") || line.starts_with("* ") {
                output.push_str(&format!("• {}\n", &line[2..]));
            } else {
                output.push_str(&line);
                output.push('\n');
            }
        }

        output
    }
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Component for rendering tool call cards
pub struct ToolCallRenderer;

impl ToolCallRenderer {
    pub fn render_tool_start(name: &str, args: &serde_json::Value) -> String {
        let args_str = serde_json::to_string_pretty(args).unwrap_or_default();
        format!(
            "╭── {} ──\n│ Arguments: {}\n",
            name,
            args_str.lines().next().unwrap_or(""),
        )
    }

    pub fn render_tool_output(output: &str) -> String {
        let lines: Vec<&str> = output.lines().take(10).collect();
        format!("│ Output:\n{}\n╰─────────\n", lines.join("\n"),)
    }

    pub fn render_tool_error(error: &str) -> String {
        format!(
            "╰── Error: {}\n",
            error.lines().next().unwrap_or("Unknown error"),
        )
    }
}

/// Editor component with autocomplete support
pub struct EditorComponent {
    content: String,
    cursor_pos: usize,
    suggestions: Vec<String>,
}

impl EditorComponent {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            cursor_pos: 0,
            suggestions: Vec::new(),
        }
    }

    pub fn insert(&mut self, text: &str) {
        self.content.insert_str(self.cursor_pos, text);
        self.cursor_pos += text.len();
        self.update_suggestions();
    }

    pub fn delete(&mut self) {
        if self.cursor_pos < self.content.len() {
            self.content.remove(self.cursor_pos);
            self.update_suggestions();
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.content.remove(self.cursor_pos);
            self.update_suggestions();
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let new_pos = (self.cursor_pos as isize + delta).max(0) as usize;
        self.cursor_pos = new_pos.min(self.content.len());
        self.update_suggestions();
    }

    fn update_suggestions(&mut self) {
        // Simple word-based suggestions
        let word = self.current_word();
        if word.len() >= 2 {
            self.suggestions = get_completions(word);
        } else {
            self.suggestions.clear();
        }
    }

    fn current_word(&self) -> &str {
        let before = &self.content[..self.cursor_pos];
        before.split_whitespace().last().unwrap_or("")
    }

    pub fn accept_suggestion(&mut self, idx: usize) {
        if idx < self.suggestions.len() {
            let word = self.current_word();
            let suggestion = self.suggestions[idx].clone();
            let replace_len = word.len();

            for _ in 0..replace_len {
                self.backspace();
            }

            self.insert(&suggestion);
        }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn suggestions(&self) -> &[String] {
        &self.suggestions
    }
}

impl Default for EditorComponent {
    fn default() -> Self {
        Self::new()
    }
}

/// Get word completions
fn get_completions(prefix: &str) -> Vec<String> {
    let completions = vec![
        "pi_read", "pi_write", "pi_edit", "pi_bash", "pi_grep", "pi_find", "pi_ls", "function",
        "struct", "impl", "mod", "async", "await", "tokio", "serde",
    ];

    completions
        .into_iter()
        .filter(|c| c.starts_with(prefix))
        .map(|c| c.to_string())
        .take(5)
        .collect()
}
