//! Codex-Style TUI - Dark, Minimalist, Terminal Aesthetic
//!
//! Inspired by OpenAI Codex CLI v0.105.0
//! Deep black background, high-contrast text, monospace, clean borders

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

// ── Color Palette (Codex Dark Theme) ─────────────────────────────────────

pub struct CodexTheme {
    pub bg: Color,          // Deep black
    pub bg_surface: Color,  // Slightly lighter black for panels
    pub fg: Color,          // Light gray/white text
    pub fg_dim: Color,      // Dimmed text
    pub accent_cyan: Color, // Interactive elements
    pub accent_blue: Color, // Links/highlights
    pub border: Color,      // Subtle borders
    pub prompt: Color,      // >_ prompt color
}

impl CodexTheme {
    pub fn codex_dark() -> Self {
        Self {
            bg: Color::Rgb(13, 13, 13),            // #0d0d0d - Deep black
            bg_surface: Color::Rgb(26, 26, 26),    // #1a1a1a - Surface
            fg: Color::Rgb(229, 229, 229),         // #e5e5e5 - Off-white
            fg_dim: Color::Rgb(140, 140, 140),     // #8c8c8c - Dimmed
            accent_cyan: Color::Rgb(6, 182, 212),  // #06b6d4 - Cyan
            accent_blue: Color::Rgb(59, 130, 246), // #3b82f6 - Blue
            border: Color::Rgb(64, 64, 64),        // #404040 - Border
            prompt: Color::Rgb(34, 197, 94),       // #22c55e - Green prompt
        }
    }
}

// ── Main Layout ──────────────────────────────────────────────────────────

/// Draw the complete Codex-style interface
pub fn draw_codex_interface<'a>(
    f: &mut Frame<'a>,
    area: Rect,
    agent_name: &str,
    version: &str,
    model: &str,
    current_dir: &str,
    input_text: &str,
    messages: &[(String, String)], // (role, content)
    state: Option<&CodexState>,
) {
    let theme = CodexTheme::codex_dark();

    // Clear background
    f.render_widget(Block::default().style(Style::default().bg(theme.bg)), area);

    // Main vertical layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6), // Header panel
            Constraint::Length(3), // Tip banner
            Constraint::Min(10),   // Chat area
            Constraint::Length(3), // Input bar
            Constraint::Length(1), // Status bar
        ])
        .margin(1)
        .split(area);

    draw_header_panel(
        f,
        chunks[0],
        agent_name,
        version,
        model,
        current_dir,
        &theme,
    );
    draw_tip_banner(f, chunks[1], &theme, model);

    // Draw chat area with thinking state if available
    let is_thinking = state.map(|s| s.is_thinking).unwrap_or(false);
    let thinking_indicator = state.map(|s| s.thinking_indicator());
    draw_chat_area_with_thinking(
        f,
        chunks[2],
        messages,
        &theme,
        is_thinking,
        thinking_indicator,
    );

    draw_input_bar(f, chunks[3], input_text, &theme, is_thinking);
    draw_status_bar(f, chunks[4], model, current_dir, &theme, is_thinking);

    // Draw autocomplete popup if state is provided and autocomplete is showing
    if let Some(state) = state {
        draw_autocomplete_popup(f, chunks[3], state, &theme);
    }
}

// ── Header Panel ─────────────────────────────────────────────────────────

fn draw_header_panel<'a>(
    f: &mut Frame<'a>,
    area: Rect,
    agent_name: &str,
    version: &str,
    model: &str,
    current_dir: &str,
    theme: &CodexTheme,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.bg_surface));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Header content
    let header_text = Text::from(vec![
        // Line 1: Prompt symbol + Agent name + version
        Line::from(vec![
            Span::styled(
                ">_ ",
                Style::default()
                    .fg(theme.prompt)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} ", agent_name),
                Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("({})", version), Style::default().fg(theme.fg_dim)),
        ]),
        // Line 2: Model info
        Line::from(vec![
            Span::styled("model: ", Style::default().fg(theme.fg_dim)),
            Span::styled(model, Style::default().fg(theme.fg)),
            Span::raw(" "),
            Span::styled(
                "/model",
                Style::default()
                    .fg(theme.accent_cyan)
                    .add_modifier(Modifier::UNDERLINED),
            ),
        ]),
        // Line 3: Directory
        Line::from(vec![
            Span::styled("directory: ", Style::default().fg(theme.fg_dim)),
            Span::styled(current_dir, Style::default().fg(theme.fg)),
        ]),
    ]);

    let header = Paragraph::new(header_text).alignment(Alignment::Left);

    f.render_widget(header, inner);
}

// ── Tip Banner ───────────────────────────────────────────────────────────

fn draw_tip_banner<'a>(f: &mut Frame<'a>, area: Rect, theme: &CodexTheme, model: &str) {
    // Get a relevant tip based on the current model
    let tip_text = get_model_tip(model);

    // Create a subtle highlighted box for the tip
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.bg_surface));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let paragraph = Paragraph::new(Line::from(vec![
        Span::styled("💡 ", Style::default().fg(theme.accent_cyan)),
        Span::styled(tip_text, Style::default().fg(theme.fg)),
    ]))
    .alignment(Alignment::Center);

    f.render_widget(paragraph, inner);
}

/// Get a helpful tip based on the model being used
fn get_model_tip(model: &str) -> &'static str {
    let model_lower = model.to_lowercase();

    if model_lower.contains("claude") {
        "Claude excels at analysis and reasoning. Try asking for detailed explanations or code review."
    } else if model_lower.contains("kimi") || model_lower.contains("moonshot") {
        "Kimi has a large context window. Perfect for long documents and multi-file analysis."
    } else if model_lower.contains("gpt") || model_lower.contains("openai") {
        "GPT models are great for creative tasks and general purpose assistance."
    } else if model_lower.contains("llama") || model_lower.contains("local") {
        "Running locally with Ollama. Your data stays on your machine - fully private!"
    } else if model_lower.contains("qwen") {
        "Qwen models are efficient and great for coding tasks. Try asking for code generation."
    } else if model_lower.contains("codex") {
        "Codex is optimized for coding. Use it for refactoring, debugging, and code review."
    } else {
        "Tip: Type /help to see available commands, /model to switch LLM providers"
    }
}

// ── Chat Area ────────────────────────────────────────────────────────────


fn draw_chat_area_with_thinking<'a>(
    f: &mut Frame<'a>,
    area: Rect,
    messages: &[(String, String)],
    theme: &CodexTheme,
    is_thinking: bool,
    thinking_indicator: Option<&'static str>,
) {
    let mut all_lines: Vec<Line> = Vec::new();

    for (role, content) in messages {
        let is_user = role == "user";
        let prefix = if is_user { "> " } else { "  " };
        let color = if is_user { theme.accent_cyan } else { theme.fg };

        // Split content by newlines and create a line for each
        let content_lines: Vec<&str> = content.split('\n').collect();

        for (idx, line_content) in content_lines.iter().enumerate() {
            let line_prefix = if idx == 0 { prefix } else { "   " };
            all_lines.push(Line::from(vec![
                Span::styled(line_prefix, Style::default().fg(theme.fg_dim)),
                Span::styled(line_content.to_string(), Style::default().fg(color)),
            ]));
        }

        // Add empty line after each message
        all_lines.push(Line::from(""));
    }

    // Add thinking indicator if agent is processing
    if is_thinking {
        let spinner = thinking_indicator.unwrap_or("⠋");
        all_lines.push(Line::from(vec![
            Span::styled("  ", Style::default().fg(theme.fg_dim)),
            Span::styled(
                spinner,
                Style::default()
                    .fg(theme.accent_cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default()),
            Span::styled("thinking...", Style::default().fg(theme.fg_dim)),
        ]));
    }

    let chat = Paragraph::new(Text::from(all_lines))
        .alignment(Alignment::Left)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .style(Style::default().bg(theme.bg));

    f.render_widget(chat, area);
}

// ── Input Bar ───────────────────────────────────────────────────────────

fn draw_input_bar<'a>(
    f: &mut Frame<'a>,
    area: Rect,
    input_text: &str,
    theme: &CodexTheme,
    is_thinking: bool,
) {
    // Dark gray bar background
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.bg_surface));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // If thinking, show a disabled input style
    let (input_line, cursor) = if is_thinking {
        (
            Line::from(vec![
                Span::styled("> ", Style::default().fg(theme.fg_dim)),
                Span::styled(
                    "(waiting for response...)",
                    Style::default().fg(theme.fg_dim),
                ),
            ]),
            Span::styled("", Style::default()), // No cursor when thinking
        )
    } else {
        (
            Line::from(vec![
                Span::styled(
                    "> ",
                    Style::default()
                        .fg(theme.prompt)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(input_text, Style::default().fg(theme.fg)),
            ]),
            Span::styled("▋", Style::default().fg(theme.accent_cyan)), // Cursor
        )
    };

    let input = Paragraph::new(input_line);
    f.render_widget(input, inner);

    // Only draw cursor if not thinking
    if !is_thinking {
        // Calculate cursor position
        let cursor_x = inner.x + 2 + input_text.len() as u16;
        let cursor_y = inner.y;
        if cursor_x < inner.x + inner.width {
            f.render_widget(Paragraph::new(cursor), Rect::new(cursor_x, cursor_y, 1, 1));
        }
    }
}

// ── Status Bar ───────────────────────────────────────────────────────────

fn draw_status_bar<'a>(
    f: &mut Frame<'a>,
    area: Rect,
    model: &str,
    current_dir: &str,
    theme: &CodexTheme,
    is_thinking: bool,
) {
    let status_text = if is_thinking {
        format!("{} · thinking... · {}", model, current_dir)
    } else {
        format!("{} · ready · {}", model, current_dir)
    };

    let status = Paragraph::new(Line::from(vec![Span::styled(
        &status_text,
        Style::default().fg(theme.fg_dim),
    )]))
    .alignment(Alignment::Center);

    f.render_widget(status, area);
}

// ── Alternative: Compact Layout ──────────────────────────────────────────

/// More compact version for smaller terminals
pub fn draw_codex_compact<'a>(
    f: &mut Frame<'a>,
    area: Rect,
    agent_name: &str,
    model: &str,
    input_text: &str,
) {
    let theme = CodexTheme::codex_dark();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Minimal header
            Constraint::Min(5),    // Content
            Constraint::Length(3), // Input
            Constraint::Length(1), // Status
        ])
        .split(area);

    // Minimal header: just >_ Agent | model
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            ">_ ",
            Style::default()
                .fg(theme.prompt)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            agent_name,
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled(model, Style::default().fg(theme.accent_cyan)),
    ]));
    f.render_widget(header, chunks[0]);

    // Input bar
    draw_input_bar(f, chunks[2], input_text, &theme, false);

    // Minimal status
    let status = Paragraph::new(Line::from(vec![Span::styled(
        "Press Enter to send, Esc to quit",
        Style::default().fg(theme.fg_dim),
    )]))
    .alignment(Alignment::Center);
    f.render_widget(status, chunks[3]);
}

// ── Helper: Centered popup ───────────────────────────────────────────────

pub fn draw_centered_popup<'a>(
    f: &mut Frame<'a>,
    area: Rect,
    title: &str,
    content: &str,
    theme: &CodexTheme,
) {
    let popup_area = centered_rect(60, 40, area);

    // Clear background
    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent_cyan))
        .style(Style::default().bg(theme.bg_surface))
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(theme.fg).add_modifier(Modifier::BOLD));

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    let text = Paragraph::new(content)
        .alignment(Alignment::Center)
        .style(Style::default().fg(theme.fg));

    f.render_widget(text, inner);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// ── Autocomplete Popup ───────────────────────────────────────────────────

/// Draw autocomplete popup above the input bar
pub fn draw_autocomplete_popup<'a>(
    f: &mut Frame<'a>,
    area: Rect,
    state: &CodexState,
    theme: &CodexTheme,
) {
    if !state.show_autocomplete || state.autocomplete_suggestions.is_empty() {
        return;
    }

    let max_visible = 6;
    let visible_count = state.autocomplete_suggestions.len().min(max_visible);
    let popup_height = (visible_count + 2) as u16; // +2 for borders

    // Position popup above the input area, use full width
    let popup_width = area.width.min(60).max(40);
    let popup_area = Rect {
        x: area.x,
        y: area.y.saturating_sub(popup_height),
        width: popup_width,
        height: popup_height,
    };

    // Clear background first
    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent_cyan))
        .style(Style::default().bg(theme.bg_surface))
        .title(format!(
            " Commands ({})",
            state.autocomplete_suggestions.len()
        ))
        .title_style(Style::default().fg(theme.fg).add_modifier(Modifier::BOLD));

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    // Build lines for suggestions
    let lines: Vec<Line> = state
        .autocomplete_suggestions
        .iter()
        .take(max_visible)
        .enumerate()
        .map(|(idx, suggestion)| {
            let is_selected = idx == state.autocomplete_selected;
            let (cmd_style, desc_style) = if is_selected {
                (
                    Style::default()
                        .fg(theme.bg)
                        .bg(theme.accent_cyan)
                        .add_modifier(Modifier::BOLD),
                    Style::default().fg(theme.bg).bg(theme.accent_cyan),
                )
            } else {
                (
                    Style::default().fg(theme.accent_cyan),
                    Style::default().fg(theme.fg_dim),
                )
            };

            Line::from(vec![
                Span::styled(if is_selected { "▶ " } else { "  " }, cmd_style),
                Span::styled(&suggestion.command, cmd_style),
                Span::styled(format!(" - {}", suggestion.description), desc_style),
            ])
        })
        .collect();

    // Add hint at bottom if there are more suggestions
    let mut all_lines = lines;
    if state.autocomplete_suggestions.len() > max_visible {
        all_lines.push(Line::from(vec![Span::styled(
            format!(
                "... and {} more",
                state.autocomplete_suggestions.len() - max_visible
            ),
            Style::default().fg(theme.fg_dim),
        )]));
    }

    // Add navigation hint
    all_lines.push(Line::from(vec![
        Span::styled("↑↓ navigate", Style::default().fg(theme.fg_dim)),
        Span::styled(" | ", Style::default().fg(theme.border)),
        Span::styled("Enter select", Style::default().fg(theme.fg_dim)),
        Span::styled(" | ", Style::default().fg(theme.border)),
        Span::styled("Esc close", Style::default().fg(theme.fg_dim)),
    ]));

    let paragraph = Paragraph::new(Text::from(all_lines));
    f.render_widget(paragraph, inner);
}

// ── Event handling helper types ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum CodexEvent {
    Input(char),
    Backspace,
    Submit,
    Quit,
    ChangeModel,
    AutocompleteNext,
    AutocompletePrev,
    AutocompleteSelect,
    NoOp,
}

/// Autocomplete suggestion
#[derive(Debug, Clone)]
pub struct AutocompleteSuggestion {
    pub command: String,
    pub description: String,
}

/// Simple state management
pub struct CodexState {
    pub input: String,
    pub messages: Vec<(String, String)>,
    pub agent_name: String,
    pub model: String,
    pub version: String,
    pub current_dir: String,
    /// Current autocomplete suggestions
    pub autocomplete_suggestions: Vec<AutocompleteSuggestion>,
    /// Selected autocomplete index
    pub autocomplete_selected: usize,
    /// Whether to show autocomplete
    pub show_autocomplete: bool,
    /// Whether agent is thinking/processing
    pub is_thinking: bool,
    /// Thinking animation frame (0-3)
    pub thinking_frame: usize,
}

impl CodexState {
    pub fn new(agent_name: &str) -> Self {
        Self {
            input: String::new(),
            messages: vec![(
                "agent".to_string(),
                format!(
                    "Hi! I'm {}. What would you like to build today?",
                    agent_name
                ),
            )],
            agent_name: agent_name.to_string(),
            model: crate::ollama_client::resolve_model_from_env("llama3.2"),
            version: "v0.1.0".to_string(),
            current_dir: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "~".to_string()),
            autocomplete_suggestions: vec![],
            autocomplete_selected: 0,
            show_autocomplete: false,
            is_thinking: false,
            thinking_frame: 0,
        }
    }

    pub fn push_message(&mut self, role: &str, content: &str) {
        self.messages.push((role.to_string(), content.to_string()));
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.show_autocomplete = false;
        self.autocomplete_suggestions.clear();
    }

    /// Update autocomplete suggestions based on current input
    pub fn update_autocomplete(&mut self, suggestions: Vec<AutocompleteSuggestion>) {
        self.autocomplete_suggestions = suggestions;
        self.autocomplete_selected = 0;
        self.show_autocomplete = !self.autocomplete_suggestions.is_empty();
    }

    /// Select next autocomplete suggestion
    pub fn autocomplete_next(&mut self) {
        if !self.autocomplete_suggestions.is_empty() {
            self.autocomplete_selected =
                (self.autocomplete_selected + 1) % self.autocomplete_suggestions.len();
        }
    }

    /// Select previous autocomplete suggestion
    pub fn autocomplete_prev(&mut self) {
        if !self.autocomplete_suggestions.is_empty() {
            self.autocomplete_selected = self.autocomplete_selected.saturating_sub(1);
        }
    }

    /// Get the selected autocomplete suggestion
    pub fn get_selected_autocomplete(&self) -> Option<&AutocompleteSuggestion> {
        self.autocomplete_suggestions
            .get(self.autocomplete_selected)
    }

    /// Apply the selected autocomplete suggestion to input
    pub fn apply_autocomplete(&mut self) {
        if let Some(suggestion) = self.get_selected_autocomplete() {
            self.input = suggestion.command.clone();
            self.show_autocomplete = false;
        }
    }

    /// Set thinking state
    pub fn set_thinking(&mut self, thinking: bool) {
        self.is_thinking = thinking;
        if thinking {
            self.thinking_frame = 0;
        }
    }

    /// Advance thinking animation frame
    pub fn advance_thinking_animation(&mut self) {
        if self.is_thinking {
            self.thinking_frame = (self.thinking_frame + 1) % 4;
        }
    }

    /// Get thinking indicator text
    pub fn thinking_indicator(&self) -> &'static str {
        const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸"];
        FRAMES[self.thinking_frame]
    }
}
