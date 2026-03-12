//! Agent Hub - Modern TUI Dashboard for Personal Agent
//!
//! A lighter, well-structured UI section with excellent UX

use ratatui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap, Tabs, Gauge, Sparkline, List, ListItem},
    Frame,
};
use crate::personal::{Persona, PersonalMemory};

/// Color palette for light/modern theme
pub struct Theme {
    pub background: Color,
    pub surface: Color,
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub text: Color,
    pub text_dim: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub border: Color,
}

impl Theme {
    /// Light, airy theme
    pub fn light() -> Self {
        Self {
            background: Color::Rgb(250, 250, 252),      // Very light gray
            surface: Color::Rgb(255, 255, 255),          // White
            primary: Color::Rgb(99, 102, 241),           // Indigo
            secondary: Color::Rgb(139, 92, 246),         // Purple
            accent: Color::Rgb(236, 72, 153),            // Pink
            text: Color::Rgb(31, 41, 55),                // Dark gray
            text_dim: Color::Rgb(107, 114, 128),         // Medium gray
            success: Color::Rgb(34, 197, 94),            // Green
            warning: Color::Rgb(251, 146, 60),           // Orange
            error: Color::Rgb(239, 68, 68),              // Red
            border: Color::Rgb(229, 231, 235),           // Light border
        }
    }

    /// Dark theme (alternative)
    pub fn dark() -> Self {
        Self {
            background: Color::Rgb(17, 24, 39),          // Dark background
            surface: Color::Rgb(31, 41, 55),             // Surface
            primary: Color::Rgb(129, 140, 248),          // Light indigo
            secondary: Color::Rgb(167, 139, 250),        // Light purple
            accent: Color::Rgb(244, 114, 182),           // Light pink
            text: Color::Rgb(243, 244, 246),             // Light text
            text_dim: Color::Rgb(156, 163, 175),         // Dim text
            success: Color::Rgb(74, 222, 128),           // Light green
            warning: Color::Rgb(251, 191, 36),           // Light orange
            error: Color::Rgb(248, 113, 113),            // Light red
            border: Color::Rgb(75, 85, 99),              // Border
        }
    }
}

/// Draw the Agent Hub - main entry point
pub fn draw_agent_hub<B: Backend>(f: &mut Frame<B>, area: Rect, persona: &Persona, memory: &PersonalMemory) {
    let theme = Theme::light();
    
    // Main layout: header + content + input
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),  // Agent header card
            Constraint::Min(10),    // Chat/content area
            Constraint::Length(3),  // Input area
        ])
        .margin(1)
        .split(area);

    // Draw components
    draw_agent_header(f, main_layout[0], persona, &theme);
    draw_chat_area(f, main_layout[1], &theme);
    draw_input_area(f, main_layout[2], &theme);
}

/// Draw agent identity header card
fn draw_agent_header<B: Backend>(f: &mut Frame<B>, area: Rect, persona: &Persona, theme: &Theme) {
    // Create a styled card block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.surface))
        .title(format!(" {} ", persona.name))
        .title_alignment(Alignment::Center)
        .title_style(Style::default()
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split inner area for content
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(inner);

    // Left: Agent info
    let info_text = Text::from(vec![
        Line::from(vec![
            Span::styled("Role: ", Style::default().fg(theme.text_dim)),
            Span::styled(&persona.identity, Style::default().fg(theme.text)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Voice: ", Style::default().fg(theme.text_dim)),
            Span::styled(&persona.voice.tone, Style::default().fg(theme.secondary)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Capabilities: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                format!("{} enabled", persona.capabilities.iter().filter(|c| c.enabled).count()),
                Style::default().fg(theme.success),
            ),
        ]),
    ]);

    let info = Paragraph::new(info_text)
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Left);
    f.render_widget(info, chunks[0]);

    // Right: Status indicators
    let status_items = vec![
        ("Memory", memory.memory_md.facts.len(), theme.primary),
        ("Projects", memory.memory_md.projects.len(), theme.secondary),
        ("Skills", 12, theme.accent), // Placeholder
    ];

    let status_lines: Vec<Line> = status_items
        .iter()
        .map(|(label, count, color)| {
            Line::from(vec![
                Span::styled(
                    format!("{} ", label),
                    Style::default().fg(theme.text_dim),
                ),
                Span::styled(
                    format!("{}", count),
                    Style::default()
                        .fg(*color)
                        .add_modifier(Modifier::BOLD),
                ),
            ])
        })
        .collect();

    let status = Paragraph::new(Text::from(status_lines))
        .alignment(Alignment::Right);
    f.render_widget(status, chunks[1]);
}

/// Draw main chat/content area
fn draw_chat_area<B: Backend>(f: &mut Frame<B>, area: Rect, theme: &Theme) {
    // Create chat block with subtle styling
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.background))
        .title(" Conversation ")
        .title_alignment(Alignment::Left)
        .title_style(Style::default().fg(theme.text_dim));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Sample conversation content
    let messages: Vec<(&str, &str, Color)> = vec![
        ("You", "Hello, can you help me with something?", theme.text),
        ("Agent", "Of course! I'm here to help. What would you like assistance with?", theme.primary),
        ("You", "I need to research multi-agent systems", theme.text),
        ("Agent", "I'll help you research multi-agent systems. Let me gather some information...", theme.primary),
    ];

    let chat_lines: Vec<Line> = messages
        .iter()
        .flat_map(|(sender, content, color)| {
            let sender_style = if *sender == "You" {
                Style::default().fg(theme.text_dim)
            } else {
                Style::default().fg(*color).add_modifier(Modifier::BOLD)
            };
            
            vec![
                Line::from(vec![
                    Span::styled(format!("{}: ", sender), sender_style),
                    Span::styled(*content, Style::default().fg(theme.text)),
                ]),
                Line::from(""),
            ]
        })
        .collect();

    let chat = Paragraph::new(Text::from(chat_lines))
        .wrap(Wrap { trim: true })
        .scroll((0, 0));

    f.render_widget(chat, inner);
}

/// Draw input area at bottom
fn draw_input_area<B: Backend>(f: &mut Frame<B>, area: Rect, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.primary))
        .style(Style::default().bg(theme.surface))
        .title(" Type your message... ")
        .title_style(Style::default().fg(theme.text_dim));

    let input = Paragraph::new("▋")
        .block(block)
        .style(Style::default().fg(theme.text));

    f.render_widget(input, area);
}

/// Draw sidebar with memory/context info
pub fn draw_sidebar<B: Backend>(f: &mut Frame<B>, area: Rect, memory: &PersonalMemory, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // Memory stats
            Constraint::Length(10), // Recent facts
            Constraint::Min(5),     // Active projects
        ])
        .split(area);

    // Memory stats card
    let stats_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.surface))
        .title(" Memory ")
        .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD));

    let stats_text = format!(
        "Facts: {}\nProjects: {}\nPreferences: {}",
        memory.memory_md.facts.len(),
        memory.memory_md.projects.len(),
        memory.memory_md.preferences.len()
    );

    let stats = Paragraph::new(stats_text)
        .block(stats_block)
        .style(Style::default().fg(theme.text));
    f.render_widget(stats, chunks[0]);

    // Recent facts
    let facts_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.surface))
        .title(" Recent Facts ")
        .title_style(Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD));

    let facts_items: Vec<ListItem> = memory.memory_md.facts
        .iter()
        .rev()
        .take(5)
        .map(|f| {
            ListItem::new(Line::from(vec![
                Span::styled("• ", Style::default().fg(theme.accent)),
                Span::styled(&f.content, Style::default().fg(theme.text)),
            ]))
        })
        .collect();

    let facts_list = List::new(facts_items)
        .block(facts_block);
    f.render_widget(facts_list, chunks[1]);

    // Active projects
    let projects_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.surface))
        .title(" Active Projects ")
        .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD));

    let projects_items: Vec<ListItem> = memory.memory_md.projects
        .iter()
        .filter(|p| matches!(p.status, crate::personal::ProjectStatus::Active))
        .map(|p| {
            ListItem::new(Line::from(vec![
                Span::styled("▸ ", Style::default().fg(theme.success)),
                Span::styled(&p.name, Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
            ]))
        })
        .collect();

    let projects_list = List::new(projects_items)
        .block(projects_block);
    f.render_widget(projects_list, chunks[2]);
}

/// Full layout with sidebar
pub fn draw_agent_hub_with_sidebar<B: Backend>(
    f: &mut Frame<B>,
    area: Rect,
    persona: &Persona,
    memory: &PersonalMemory,
) {
    let theme = Theme::light();

    // Split: main content (70%) + sidebar (30%)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    draw_agent_hub(f, chunks[0], persona, memory);
    draw_sidebar(f, chunks[1], memory, &theme);
}

/// Quick action bar
pub fn draw_quick_actions<B: Backend>(f: &mut Frame<B>, area: Rect, theme: &Theme) {
    let actions = vec![
        ("[Enter]", "Send", theme.primary),
        ("[Esc]", "Back", theme.text_dim),
        ("[Tab]", "Focus", theme.secondary),
        ("[Ctrl+C]", "Quit", theme.error),
    ];

    let spans: Vec<Span> = actions
        .iter()
        .flat_map(|(key, action, color)| {
            vec![
                Span::styled(*key, Style::default().fg(*color).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {} ", action), Style::default().fg(theme.text_dim)),
                Span::raw("  "),
            ]
        })
        .collect();

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line)
        .alignment(Alignment::Center);

    f.render_widget(paragraph, area);
}
