//! Codex-Style TUI Demo
//!
//! A dark, minimalist terminal interface inspired by OpenAI Codex CLI
//! Run: cargo run --bin tui_codex_demo

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    Terminal,
};
use std::io;

use hyper_stigmergy::tui_codex_style::{draw_codex_interface, CodexEvent, CodexState};

#[tokio::main]
async fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run app
    let result = run_app(&mut terminal).await;

    // Restore terminal
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;

    result
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    let mut state = CodexState::new("Ash");
    let mut last_tick = tokio::time::Instant::now();
    let tick_rate = tokio::time::Duration::from_millis(100);

    loop {
        // Draw UI
        terminal.draw(|f| {
            draw_codex_interface(
                f,
                f.size(),
                &state.agent_name,
                &state.version,
                &state.model,
                &state.current_dir,
                &state.input,
                &state.messages,
                Some(&state),
            );
        })?;

        // Handle events with timeout
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if crossterm::event::poll(timeout)? {
            match handle_event(&state).await? {
                CodexEvent::Quit => break,
                CodexEvent::Submit => {
                    if !state.input.is_empty() {
                        // Add user message
                        state.push_message("user", &state.input.clone());
                        let user_input = state.input.clone();
                        state.clear_input();

                        // Simulate agent response
                        let response = format!(
                            "I'd help you with '{}'. This is a demo - integrate with your LLM for real responses!",
                            user_input
                        );
                        state.push_message("agent", &response);
                    }
                }
                CodexEvent::Input(c) => {
                    state.input.push(c);
                }
                CodexEvent::Backspace => {
                    state.input.pop();
                }
                CodexEvent::ChangeModel => {
                    // Cycle models
                    state.model = match state.model.as_str() {
                        "llama3.2" => "claude-3.5-sonnet",
                        "claude-3.5-sonnet" => "gpt-4",
                        _ => "llama3.2",
                    }
                    .to_string();
                }
                CodexEvent::AutocompleteNext => {
                    state.autocomplete_next();
                }
                CodexEvent::AutocompletePrev => {
                    state.autocomplete_prev();
                }
                CodexEvent::AutocompleteSelect => {
                    state.apply_autocomplete();
                }
                CodexEvent::NoOp => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = tokio::time::Instant::now();
        }
    }

    Ok(())
}

async fn handle_event(state: &CodexState) -> Result<CodexEvent> {
    if let Event::Key(key) = event::read()? {
        // Handle autocomplete navigation when autocomplete is showing
        if state.show_autocomplete && !state.autocomplete_suggestions.is_empty() {
            match key.code {
                KeyCode::Down | KeyCode::Tab => return Ok(CodexEvent::AutocompleteNext),
                KeyCode::Up => return Ok(CodexEvent::AutocompletePrev),
                KeyCode::Enter | KeyCode::Right => return Ok(CodexEvent::AutocompleteSelect),
                KeyCode::Esc => {
                    // Just close autocomplete
                    return Ok(CodexEvent::NoOp);
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Ok(CodexEvent::Quit)
            }
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Ok(CodexEvent::Quit)
            }
            KeyCode::Esc => Ok(CodexEvent::Quit),
            KeyCode::Enter => Ok(CodexEvent::Submit),
            KeyCode::Char('m') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Ok(CodexEvent::ChangeModel)
            }
            KeyCode::Backspace => Ok(CodexEvent::Backspace),
            KeyCode::Char(c) => Ok(CodexEvent::Input(c)),
            _ => Ok(CodexEvent::NoOp),
        }
    } else {
        Ok(CodexEvent::NoOp)
    }
}
