use color_eyre::eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dotenvy::dotenv;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    env, fs,
    io::{self, stdout},
    time::Duration,
};

mod app;
mod llm;
mod tpm_limiter;
mod ui;

use app::AppState;
use llm::{AzureOpenAIClient, LLMProvider};
use tpm_limiter::TPMLimiter;
use ui::draw_ui;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    dotenv().ok();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = AppState::default();

    let client = AzureOpenAIClient::new()
        .map_err(|e| color_eyre::eyre::eyre!("Failed to create AzureOpenAIClient: {}", e))?;

    let prompt = fs::read_to_string("prompt.txt").unwrap_or_else(|_| {
        "You are a helpful AI coding assistant.".to_string()
    });

    let project_root = env::var("PROJECT_PATH").unwrap_or_else(|_| ".".to_string());

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
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {:?}", err);
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

        // Update spinner every 80ms for fluid animation
        if last_update.elapsed() > Duration::from_millis(80) {
            spinner_index = (spinner_index + 1) % spinner_frames.len();
            last_update = std::time::Instant::now();
        }

        if app.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.should_quit = true;
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.should_quit = true;
                        }
                        KeyCode::Enter => {
                            if !app.chat_input.trim().is_empty() && !app.processing {
                                let user_message = app.chat_input.clone();
                                app.chat_input.clear();
                                app.conversation_history
                                    .push(format!("User: {}", user_message));
                                app.processing = true;
                                process_iteration(app, client, prompt, project_root, tpm_limiter)
                                    .await?;
                                app.processing = false;
                            }
                        }
                        KeyCode::Char(c) => {
                            if !app.processing {
                                app.chat_input.push(c);
                            }
                        }
                        KeyCode::Backspace => {
                            if !app.processing {
                                app.chat_input.pop();
                            }
                        }
                        KeyCode::Up => {
                            if app.thoughts_scroll > 0 {
                                app.thoughts_scroll = app.thoughts_scroll.saturating_sub(1);
                            }
                        }
                        KeyCode::Down => {
                            let max_scroll = app.current_thoughts.lines().count().saturating_sub(10);
                            if app.thoughts_scroll < max_scroll as u32 {
                                app.thoughts_scroll += 1;
                            }
                        }
                        KeyCode::PageUp => {
                            app.thoughts_scroll = app.thoughts_scroll.saturating_sub(5);
                        }
                        KeyCode::PageDown => {
                            let max_scroll = app.current_thoughts.lines().count().saturating_sub(10);
                            app.thoughts_scroll = (app.thoughts_scroll + 5).min(max_scroll as u32);
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

    app.stats.input_tokens = app::count_tokens(&context);
    app.current_thoughts = "🤔 Thinking...".to_string();

    // Rate limiting
    tpm_limiter.wait_if_needed();
    app.stats.current_tpm = tpm_limiter.get_current_tpm();

    // LLM Request
    match client.generate(&context, &serde_json::json!({})).await {
        Ok(resp) => {
            let raw_response = resp;
            let response = app::filter_thinking_tokens(&raw_response);
            app.current_thoughts = response.clone();

            let output_tokens = app::count_tokens(&response);
            let total_tokens = app.stats.input_tokens + output_tokens;

            tpm_limiter.add_token_usage(total_tokens);
            app.stats.output_tokens = output_tokens;
            app.stats.total_tokens = tpm_limiter.get_total_tokens();
            app.stats.current_tpm = tpm_limiter.get_current_tpm();

            app.conversation_history
                .push(format!("Assistant: {}", response));

            let tools = app::extract_tools(&response);

            // Execute tools
            for (tool, param) in tools {
                let result = app::execute_tool(&tool, &param, project_root);
                app.current_tools
                    .push((tool.clone(), param.clone(), result.clone()));

                // Check for success condition
                if tool == "execute_command" && param.contains("cargo run") {
                    if result.contains("exit_code: 0") && !result.to_lowercase().contains("error") {
                        app.success_achieved = true;
                    }
                }
            }

            if !app.current_tools.is_empty() {
                let tool_summary: Vec<String> = app
                    .current_tools
                    .iter()
                    .map(|(t, p, r)| {
                        format!(
                            "{}: {} -> {}",
                            t,
                            if p.len() > 30 {
                                format!("{}...", &p[..30])
                            } else {
                                p.clone()
                            },
                            if r.len() > 50 {
                                format!("{}...", &r[..50])
                            } else {
                                r.clone()
                            }
                        )
                    })
                    .collect();

                app.conversation_history
                    .push(format!("Tool Results:\n{}", tool_summary.join("\n")));
            }

            // Keep only last 10 conversation items
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
