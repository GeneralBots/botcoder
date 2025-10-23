use color_eyre::eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dotenvy::dotenv;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{BarChart, Block, Borders, Dataset, Gauge, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use std::{
    collections::VecDeque,
    env, fs,
    io::{self, Write},
    path::Path,
    process::Command,
    time::{Duration, SystemTime},
};

mod app;
mod llm;
mod tpm_limiter;
mod ui;

use app::AppState;
use llm::AzureOpenAIClient;
use tpm_limiter::TPMLimiter;
use ui::draw_ui;

use crate::llm::LLMProvider;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = AppState::default();

    let client = AzureOpenAIClient::new()
        .map_err(|e| color_eyre::eyre::eyre!("Failed to create AzureOpenAIClient: {}", e))?;

    let prompt = fs::read_to_string("prompt.txt").expect("Failed to read prompt.txt");

    let project_root = env::var("PROJECT_PATH").expect("PROJECT_PATH not set");

    let tpm_limit: u32 = env::var("LLM_TPM")
        .unwrap_or_else(|_| "20000".to_string())
        .parse()
        .unwrap_or(20000);

    let min_interval_secs: u64 = env::var("LLM_MIN_INTERVAL")
        .unwrap_or_else(|_| "10".to_string())
        .parse()
        .unwrap_or(10);

    let mut tpm_limiter = TPMLimiter::new(tpm_limit, min_interval_secs);
    app.stats.max_tpm = tpm_limit;

    // Main loop
    let result = run_app(
        &mut terminal,
        &mut app,
        &client,
        &prompt,
        &project_root,
        &mut tpm_limiter,
    )
    .await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen,)?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        println!("{:?}", err);
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
    client: &AzureOpenAIClient,
    prompt: &str,
    project_root: &str,
    tpm_limiter: &mut TPMLimiter,
) -> Result<()> {
    // Start first iteration automatically
    process_iteration(app, client, prompt, project_root, tpm_limiter).await?;

    let mut last_update = std::time::Instant::now();
    let spinner_frames = vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let mut spinner_index = 0;

    loop {
        terminal.draw(|f| draw_ui(f, app, &spinner_frames[spinner_index]))?;

        // Update spinner every 100ms
        if last_update.elapsed() > Duration::from_millis(100) {
            spinner_index = (spinner_index + 1) % spinner_frames.len();
            last_update = std::time::Instant::now();
        }

        if app.should_quit || app.success_achieved {
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
                        break;
                    }
                }
            }
            continue;
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.should_quit = true;
                            break;
                        }
                        KeyCode::Enter => {
                            // Process user input from chat
                            if !app.chat_input.trim().is_empty() {
                                let user_message = app.chat_input.clone();
                                app.chat_input.clear();
                                app.conversation_history
                                    .push(format!("User: {}", user_message));
                                process_iteration(app, client, prompt, project_root, tpm_limiter)
                                    .await?;
                            }
                        }
                        KeyCode::Char(c) => {
                            app.chat_input.push(c);
                        }
                        KeyCode::Backspace => {
                            app.chat_input.pop();
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

async fn process_iteration(
    app: &mut AppState,
    client: &AzureOpenAIClient,
    prompt: &str,
    project_root: &str,
    tpm_limiter: &mut TPMLimiter,
) -> Result<()> {
    app.iteration += 1;
    app.current_tools.clear();

    let context = if app.conversation_history.is_empty() {
        format!("{}\n\nProject: {}\n\nConversation:", prompt, project_root)
    } else {
        format!(
            "{}\n\nProject: {}\n\nConversation History:\n{}\n\nNext:",
            prompt,
            project_root,
            app.conversation_history.join("\n\n")
        )
    };

    app.stats.input_tokens = crate::app::count_tokens(&context);
    app.current_thoughts = "🤔 Thinking...".to_string();

    // Rate limiting
    tpm_limiter.wait_if_needed();
    app.stats.current_tpm = tpm_limiter.get_current_tpm();

    // LLM Request
    match client.generate(&context, &serde_json::json!({})).await {
        Ok(resp) => {
            let raw_response = resp.to_string();
            let response = crate::app::filter_thinking_tokens(&raw_response);
            app.current_thoughts = response.clone();

            let output_tokens = crate::app::count_tokens(&response);
            let total_tokens = app.stats.input_tokens + output_tokens;

            tpm_limiter.add_token_usage(total_tokens);
            app.stats.output_tokens = output_tokens;
            app.stats.total_tokens = tpm_limiter.get_total_tokens();
            app.stats.current_tpm = tpm_limiter.get_current_tpm();

            app.conversation_history
                .push(format!("Assistant: {}", response));

            let tools = crate::app::extract_tools(&response);

            // Execute tools
            for (tool, param) in tools {
                let result = crate::app::execute_tool(&tool, &param, project_root);
                let tool_clone = tool.clone();
                let param_clone = param.clone();
                let result_clone = result.clone();
                app.current_tools
                    .push((tool_clone, param_clone, result_clone));

                // Check for success condition
                if tool == "execute_command" && param.contains("cargo run") {
                    if result.contains("exit_code: 0") && !result.to_lowercase().contains("error") {
                        app.success_achieved = true;
                    }
                }
            }

            app.conversation_history
                .push(format!("Tool Results:\n{:?}", app.current_tools));

            if app.conversation_history.len() > 10 {
                app.conversation_history
                    .drain(0..app.conversation_history.len() - 10);
            }
        }
        Err(err) => {
            app.current_thoughts = format!("❌ Error: {}", err);
        }
    }

    Ok(())
}
