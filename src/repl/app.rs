//! Envision-based TUI for the Neap REPL.
//!
//! This module implements the interactive REPL using the envision framework,
//! which provides a TEA (The Elm Architecture) pattern for TUI development.

use std::io::{BufReader, Cursor};

use envision::app::{App, Command, Runtime};
use envision::component::{Component, InputField, InputFieldState, InputMessage, InputOutput};
use envision::input::{KeyCode, KeyModifiers, SimulatedEvent};
use envision::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use super::engine::ReplEngine;
use super::response::Response;
use super::CommandResult;

/// A history entry in the REPL.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// The input line entered by the user.
    pub input: String,
    /// The output produced by evaluation.
    pub output: String,
    /// Whether this was an error.
    pub is_error: bool,
}

/// The REPL application state.
#[derive(Clone)]
pub struct ReplState {
    /// The input field state.
    input: InputFieldState,
    /// History of inputs and outputs.
    history: Vec<HistoryEntry>,
    /// Current scroll position in history (0 = bottom).
    scroll_offset: usize,
    /// Whether the application should quit.
    should_quit: bool,
    /// Pending output from evaluation (since we can't store engine in Clone state).
    pending_eval: Option<String>,
    /// Current position in command history for Up/Down navigation.
    /// None means we're at the current (new) input position.
    history_nav_index: Option<usize>,
    /// Saved input before starting history navigation.
    saved_input: String,
}

impl Default for ReplState {
    fn default() -> Self {
        let mut input = InputFieldState::default();
        input.set_placeholder("Enter expression or :help");
        Self {
            input,
            history: Vec::new(),
            scroll_offset: 0,
            should_quit: false,
            pending_eval: None,
            history_nav_index: None,
            saved_input: String::new(),
        }
    }
}

/// Messages for the REPL application.
#[derive(Debug, Clone)]
pub enum ReplMsg {
    /// Input field message.
    Input(InputMessage),
    /// Submit the current input.
    Submit,
    /// Navigate to previous command in history (Up arrow).
    HistoryPrev,
    /// Navigate to next command in history (Down arrow).
    HistoryNext,
    /// Scroll output view up (Shift+Up or PageUp).
    ScrollUp,
    /// Scroll output view down (Shift+Down or PageDown).
    ScrollDown,
    /// Clear the screen/history.
    Clear,
    /// Quit the application.
    Quit,
    /// Add a history entry (from external evaluation).
    AddHistory(HistoryEntry),
}

/// The Neap REPL application.
pub struct NeapReplApp;

impl App for NeapReplApp {
    type State = ReplState;
    type Message = ReplMsg;

    fn init() -> (Self::State, Command<Self::Message>) {
        (ReplState::default(), Command::none())
    }

    fn update(state: &mut Self::State, msg: Self::Message) -> Command<Self::Message> {
        match msg {
            ReplMsg::Input(input_msg) => {
                // Handle input field messages
                if let Some(output) = InputField::update(&mut state.input, input_msg) {
                    match output {
                        InputOutput::Submitted(_) => {
                            return Command::message(ReplMsg::Submit);
                        }
                        InputOutput::Changed(_) => {
                            // Reset history navigation when user types
                            state.history_nav_index = None;
                            state.saved_input.clear();
                        }
                    }
                }
            }
            ReplMsg::Submit => {
                let input = state.input.value().to_string();
                if !input.is_empty() {
                    // Store the input for processing by the external loop
                    state.pending_eval = Some(input);
                    state.input.set_value("");
                    // Reset history navigation
                    state.history_nav_index = None;
                    state.saved_input.clear();
                }
            }
            ReplMsg::HistoryPrev => {
                if state.history.is_empty() {
                    return Command::none();
                }

                match state.history_nav_index {
                    None => {
                        // Starting navigation - save current input and go to last history item
                        state.saved_input = state.input.value().to_string();
                        let idx = state.history.len() - 1;
                        state.history_nav_index = Some(idx);
                        state.input.set_value(&state.history[idx].input);
                    }
                    Some(idx) if idx > 0 => {
                        // Go to previous (older) item
                        let new_idx = idx - 1;
                        state.history_nav_index = Some(new_idx);
                        state.input.set_value(&state.history[new_idx].input);
                    }
                    Some(_) => {
                        // Already at oldest item, do nothing
                    }
                }
            }
            ReplMsg::HistoryNext => {
                match state.history_nav_index {
                    None => {
                        // Not navigating, do nothing
                    }
                    Some(idx) if idx < state.history.len() - 1 => {
                        // Go to next (newer) item
                        let new_idx = idx + 1;
                        state.history_nav_index = Some(new_idx);
                        state.input.set_value(&state.history[new_idx].input);
                    }
                    Some(_) => {
                        // At newest history item, go back to saved input
                        state.history_nav_index = None;
                        state.input.set_value(&state.saved_input);
                        state.saved_input.clear();
                    }
                }
            }
            ReplMsg::ScrollUp => {
                let max_scroll = state.history.len().saturating_sub(1);
                state.scroll_offset = (state.scroll_offset + 1).min(max_scroll);
            }
            ReplMsg::ScrollDown => {
                state.scroll_offset = state.scroll_offset.saturating_sub(1);
            }
            ReplMsg::Clear => {
                state.history.clear();
                state.scroll_offset = 0;
                state.history_nav_index = None;
                state.saved_input.clear();
            }
            ReplMsg::Quit => {
                state.should_quit = true;
            }
            ReplMsg::AddHistory(entry) => {
                // Check for quit command
                if entry.output == "QUIT" {
                    state.should_quit = true;
                } else {
                    state.history.push(entry);
                    state.scroll_offset = 0; // Scroll to bottom
                }
            }
        }
        Command::none()
    }

    fn view(state: &Self::State, frame: &mut Frame) {
        let area = frame.area();

        // Layout: history area at top, input at bottom
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),    // History
                Constraint::Length(3), // Input
            ])
            .split(area);

        // Render history
        render_history(state, frame, chunks[0]);

        // Render input field
        render_input(state, frame, chunks[1]);
    }

    fn handle_event(_state: &Self::State, event: &SimulatedEvent) -> Option<Self::Message> {
        match event {
            SimulatedEvent::Key(key) => {
                // Handle Ctrl+C and Ctrl+D for quit
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match key.code {
                        KeyCode::Char('c') | KeyCode::Char('d') => {
                            return Some(ReplMsg::Quit);
                        }
                        _ => {}
                    }
                }

                // Handle special keys
                match key.code {
                    KeyCode::Enter => Some(ReplMsg::Submit),
                    // Up/Down for command history navigation (like readline)
                    KeyCode::Up if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                        Some(ReplMsg::HistoryPrev)
                    }
                    KeyCode::Down if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                        Some(ReplMsg::HistoryNext)
                    }
                    // Shift+Up/Down or PageUp/PageDown for scrolling output
                    KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        Some(ReplMsg::ScrollUp)
                    }
                    KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        Some(ReplMsg::ScrollDown)
                    }
                    KeyCode::PageUp => Some(ReplMsg::ScrollUp),
                    KeyCode::PageDown => Some(ReplMsg::ScrollDown),
                    _ => {
                        // Get input message from the key event
                        event_to_input_message(key).map(ReplMsg::Input)
                    }
                }
            }
            _ => None,
        }
    }

    fn should_quit(state: &Self::State) -> bool {
        state.should_quit
    }
}

/// Convert a key event to an input message.
fn event_to_input_message(key: &envision::input::KeyEvent) -> Option<InputMessage> {
    match key.code {
        KeyCode::Char(c) => Some(InputMessage::Insert(c)),
        KeyCode::Backspace => Some(InputMessage::Backspace),
        KeyCode::Delete => Some(InputMessage::Delete),
        KeyCode::Left => Some(InputMessage::Left),
        KeyCode::Right => Some(InputMessage::Right),
        KeyCode::Home => Some(InputMessage::Home),
        KeyCode::End => Some(InputMessage::End),
        _ => None,
    }
}

/// Render the history section.
fn render_history(state: &ReplState, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .title(" Neap REPL v0.1.0 ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.history.is_empty() {
        let welcome = Paragraph::new(vec![
            Line::from("Welcome to the Neap REPL!"),
            Line::from(""),
            Line::from("Type expressions to evaluate, or :help for commands."),
            Line::from("Press Ctrl+C or :quit to exit."),
        ])
        .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(welcome, inner);
        return;
    }

    // Build lines from history
    let mut lines: Vec<Line> = Vec::new();
    for entry in &state.history {
        // Input line with prompt
        lines.push(Line::from(vec![
            Span::styled("neap> ", Style::default().fg(Color::Cyan)),
            Span::raw(&entry.input),
        ]));

        // Output line(s)
        let output_style = if entry.is_error {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };

        for output_line in entry.output.lines() {
            lines.push(Line::from(Span::styled(output_line, output_style)));
        }

        // Blank line between entries
        lines.push(Line::from(""));
    }

    // Calculate visible window based on scroll
    let visible_height = inner.height as usize;
    let total_lines = lines.len();
    let start = total_lines.saturating_sub(visible_height + state.scroll_offset);
    let end = total_lines.saturating_sub(state.scroll_offset);

    let visible_lines: Vec<Line> = lines.into_iter().skip(start).take(end - start).collect();

    let paragraph = Paragraph::new(visible_lines);
    frame.render_widget(paragraph, inner);
}

/// Render the input section.
fn render_input(state: &ReplState, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .title(" Input ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Render input with prompt
    let prompt = Span::styled("neap> ", Style::default().fg(Color::Cyan));
    let input_value = state.input.value();
    let cursor_pos = state.input.cursor_position();

    let placeholder = state.input.placeholder();
    let input_text = if input_value.is_empty() && !placeholder.is_empty() {
        Span::styled(placeholder, Style::default().fg(Color::DarkGray))
    } else {
        Span::raw(input_value)
    };

    let line = Line::from(vec![prompt, input_text]);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, inner);

    // Set cursor position
    frame.set_cursor_position(Position::new(
        inner.x + 6 + cursor_pos as u16, // 6 = "neap> " length
        inner.y,
    ));
}

/// Run the TUI REPL.
///
/// This is the main entry point for the envision-based REPL.
pub fn run_tui() -> Result<(), Box<dyn std::error::Error>> {
    // Create the engine (can't be in state due to Clone requirement)
    let mut engine: ReplEngine<Vec<u8>, BufReader<Cursor<Vec<u8>>>> =
        ReplEngine::new(Vec::new(), BufReader::new(Cursor::new(Vec::new())));

    // Create runtime with crossterm backend
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut runtime = Runtime::<NeapReplApp, _>::with_backend(backend)?;

    // Main loop with manual tick
    loop {
        // Check for pending evaluation before tick
        if let Some(input) = runtime.state().pending_eval.clone() {
            let entry = evaluate_input(&mut engine, &input);
            // Clear pending_eval and add the result
            runtime.state_mut().pending_eval = None;
            let cmd = NeapReplApp::update(runtime.state_mut(), ReplMsg::AddHistory(entry));
            // Process the command if any
            if !cmd.is_none() {
                // Commands would be handled here if needed
            }
        }

        // Run one tick (processes events, updates state, renders)
        runtime.tick()?;

        if NeapReplApp::should_quit(runtime.state()) {
            break;
        }
    }

    Ok(())
}

/// Evaluate input using the engine and return a history entry.
fn evaluate_input<W: std::io::Write, R: std::io::BufRead>(
    engine: &mut ReplEngine<W, R>,
    input: &str,
) -> HistoryEntry {
    let input = input.trim();

    // Handle commands
    if input.starts_with(':') {
        let result = engine.eval_command(input);
        match result {
            CommandResult::Quit => HistoryEntry {
                input: input.to_string(),
                output: "QUIT".to_string(),
                is_error: false,
            },
            CommandResult::Help(text) => HistoryEntry {
                input: input.to_string(),
                output: text,
                is_error: false,
            },
            CommandResult::Cleared => HistoryEntry {
                input: input.to_string(),
                output: "Environment cleared.".to_string(),
                is_error: false,
            },
            CommandResult::TypeOf { ty } => HistoryEntry {
                input: input.to_string(),
                output: ty.to_string(),
                is_error: false,
            },
            CommandResult::Unknown { cmd } => HistoryEntry {
                input: input.to_string(),
                output: cmd,
                is_error: true,
            },
        }
    } else {
        // Evaluate expression or declaration
        match engine.eval(input) {
            Ok(Some(result)) => {
                let response = Response::from_eval_result(result);
                HistoryEntry {
                    input: input.to_string(),
                    output: response.text().to_string(),
                    is_error: response.is_error(),
                }
            }
            Ok(None) => HistoryEntry {
                input: input.to_string(),
                output: "Error: incomplete input".to_string(),
                is_error: true,
            },
            Err(e) => HistoryEntry {
                input: input.to_string(),
                output: format!("Error: {e}"),
                is_error: true,
            },
        }
    }
}
