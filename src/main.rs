use dotenvy::dotenv;
use std::collections::VecDeque;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime};

mod llm;
use llm::AzureOpenAIClient;

use crate::llm::LLMProvider;

// Color constants
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";
const WHITE: &str = "\x1b[37m";
const BG_BLUE: &str = "\x1b[44m";
const BG_GREEN: &str = "\x1b[42m";
const BG_RED: &str = "\x1b[41m";
const BG_YELLOW: &str = "\x1b[43m";
const BG_MAGENTA: &str = "\x1b[45m";
const BLACK: &str = "\x1b[30m";

struct TPMLimiter {
    max_tpm: u32,
    min_interval: Duration,
    token_usage: VecDeque<(SystemTime, u32)>,
    last_request: Option<SystemTime>,
    total_tokens_used: u32,
}

impl TPMLimiter {
    fn new(max_tpm: u32, min_interval_secs: u64) -> Self {
        Self {
            max_tpm,
            min_interval: Duration::from_secs(min_interval_secs),
            token_usage: VecDeque::new(),
            last_request: None,
            total_tokens_used: 0,
        }
    }

    fn add_token_usage(&mut self, tokens: u32) {
        let now = SystemTime::now();
        self.token_usage.push_back((now, tokens));
        self.total_tokens_used += tokens;
        self.last_request = Some(now);

        // Clean up old entries (older than 1 minute)
        let one_minute_ago = now - Duration::from_secs(60);
        while let Some(front) = self.token_usage.front() {
            if front.0 < one_minute_ago {
                self.token_usage.pop_front();
            } else {
                break;
            }
        }
    }

    fn wait_if_needed(&mut self) {
        if let Some(last_request) = self.last_request {
            if let Ok(elapsed) = last_request.elapsed() {
                if elapsed < self.min_interval {
                    let sleep_time = self.min_interval - elapsed;
                    thread::sleep(sleep_time);
                }
            }
        }

        let current_tpm = self.get_current_tpm();
        if current_tpm >= self.max_tpm {
            let wait_time = Duration::from_secs(60);
            println!(
                "\n{}TPM limit reached, waiting for reset...{}",
                YELLOW, RESET
            );
            thread::sleep(wait_time);
        }
    }

    fn get_current_tpm(&self) -> u32 {
        let now = SystemTime::now();
        let one_minute_ago = now - Duration::from_secs(60);

        self.token_usage
            .iter()
            .filter(|(time, _)| *time >= one_minute_ago)
            .map(|(_, tokens)| tokens)
            .sum()
    }

    fn get_total_tokens(&self) -> u32 {
        self.total_tokens_used
    }
}

struct ConsoleUI {
    width: usize,
    height: usize,
}

impl ConsoleUI {
    fn new() -> Self {
        let (width, height) = Self::get_terminal_size();
        Self { width, height }
    }

    fn get_terminal_size() -> (usize, usize) {
        #[cfg(unix)]
        {
            use libc::{ioctl, winsize, STDOUT_FILENO, TIOCGWINSZ};
            unsafe {
                let mut winsize = winsize {
                    ws_row: 0,
                    ws_col: 0,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                if ioctl(STDOUT_FILENO, TIOCGWINSZ, &mut winsize) == 0 {
                    (winsize.ws_col as usize, winsize.ws_row as usize)
                } else {
                    (80, 24)
                }
            }
        }
        #[cfg(not(unix))]
        {
            (80, 24)
        }
    }

    fn clear_screen(&self) {
        print!("\x1B[2J\x1B[1;1H");
    }

    fn draw_box(&self, x: usize, y: usize, width: usize, height: usize, title: &str, color: &str) {
        // Move cursor to position
        print!("\x1B[{};{}H", y, x);

        // Top border with title
        print!(
            "{}{}┌{}{:─<width$}┐{}",
            color,
            BOLD,
            title,
            "",
            RESET,
            width = width - 2 - title.chars().count()
        );

        // Sides
        for i in 1..height - 1 {
            print!("\x1B[{};{}H", y + i, x);
            print!("{}│{}", color, RESET);
            print!("\x1B[{};{}H", y + i, x + width - 1);
            print!("{}│{}", color, RESET);
        }

        // Bottom border
        print!("\x1B[{};{}H", y + height - 1, x);
        print!("{}└{:─<width$}┘{}", color, "", RESET, width = width - 2);
    }

    fn draw_header(&self) {
        self.clear_screen();

        // Top bar
        println!("{}{}{}{}", BG_BLUE, BLACK, "▄".repeat(self.width), RESET);

        // Title box
        let title = "General Bots Coder";
        let title_width = title.chars().count();
        let padding = (self.width.saturating_sub(title_width + 4)) / 2;

        println!(
            "{}{}┌{:─<width$}┐{}",
            BG_BLUE,
            BOLD,
            "",
            RESET,
            width = self.width - 2
        );
        println!(
            "{}│{:^width$}│{}",
            BG_BLUE,
            format!("{}{}{}", MAGENTA, BOLD, title),
            RESET,
            width = self.width - 2
        );
        println!(
            "{}└{:─<width$}┘{}",
            BG_BLUE,
            "",
            RESET,
            width = self.width - 2
        );
        println!();
    }

    fn draw_status_bar(&self, iteration: u32, total_tokens: u32, current_tpm: u32, max_tpm: u32) {
        let status = format!(
            "Iteration: {} | Tokens: {} | TPM: {}/{}",
            iteration, total_tokens, current_tpm, max_tpm
        );

        println!(
            "{}{}{}{:^width$}{}",
            BG_GREEN,
            BLACK,
            BOLD,
            status,
            RESET,
            width = self.width
        );
    }

    fn draw_content_box(
        &self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        title: &str,
        content: &str,
        color: &str,
    ) {
        self.draw_box(x, y, width, height, title, color);

        // Split content into lines and display within box
        let content_lines: Vec<&str> = content.lines().collect();
        let max_lines = height.saturating_sub(2);

        for (i, line) in content_lines.iter().take(max_lines).enumerate() {
            let display_line = if line.chars().count() > width - 4 {
                format!("{}...", &line[..width - 7])
            } else {
                line.to_string()
            };

            print!("\x1B[{};{}H", y + i + 1, x + 2);
            print!("{}{}", color, display_line);
        }

        if content_lines.len() > max_lines {
            print!("\x1B[{};{}H", y + max_lines + 1, x + 2);
            print!(
                "{}... ({} more lines){}",
                YELLOW,
                content_lines.len() - max_lines,
                RESET
            );
        }

        print!("{}", RESET);
    }
}

fn count_tokens(text: &str) -> u32 {
    text.split_whitespace().count() as u32
}

fn filter_thinking_tokens(text: &str) -> String {
    text.replace("<|start|>assistant<|channel|>", "")
        .replace("<|message|>", "")
        .replace("<|end|>", "")
        .trim()
        .to_string()
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    let ui = ConsoleUI::new();
    ui.draw_header();

    let instruction =
        env::var("INSTRUCTION").expect("Please set the INSTRUCTION variable in your .env file");

    let client = match AzureOpenAIClient::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}Failed to create AzureOpenAIClient: {}{}", RED, e, RESET);
            return;
        }
    };

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

    // Display configuration
    println!(
        "{}╔═════════════════════════════════════════════════════════════╗{}",
        BLUE, RESET
    );
    println!("{}║ {:^59} {}║{}", BLUE, "⚙️  CONFIGURATION", BLUE, RESET);
    println!(
        "{}╠═════════════════════════════════════════════════════════════╣{}",
        BLUE, RESET
    );
    println!(
        "{}║ {:<15}: {:<40} {}║{}",
        BLUE, "Project Path", project_root, BLUE, RESET
    );
    println!(
        "{}║ {:<15}: {:<6} tokens/minute               {}║{}",
        BLUE, "Max TPM", tpm_limit, BLUE, RESET
    );
    println!(
        "{}║ {:<15}: {:<6} seconds                     {}║{}",
        BLUE, "Min Interval", min_interval_secs, BLUE, RESET
    );
    println!(
        "{}╚═════════════════════════════════════════════════════════════╝{}",
        BLUE, RESET
    );
    println!();

    let mut iteration = 0;
    let mut conversation_history: Vec<String> = Vec::new();

    loop {
        iteration += 1;
        ui.clear_screen();
        ui.draw_header();

        // Status bar
        let current_tpm = tpm_limiter.get_current_tpm();
        let total_tokens = tpm_limiter.get_total_tokens();
        ui.draw_status_bar(iteration, total_tokens, current_tpm, tpm_limit);
        println!();

        // Iteration header
        println!(
            "{}╔═════════════════════════════════════════════════════════════╗{}",
            MAGENTA, RESET
        );
        println!(
            "{}║ {:^59} {}║{}",
            MAGENTA,
            &format!("🚀 ITERATION {}", iteration),
            MAGENTA,
            RESET
        );
        println!(
            "{}╚═════════════════════════════════════════════════════════════╝{}",
            MAGENTA, RESET
        );
        println!();

        let context = if conversation_history.is_empty() {
            format!(
                "{}\n\nProject: {}\nTask: {}\n\nNext step:",
                prompt, project_root, instruction
            )
        } else {
            format!(
                "{}\n\nProject: {}\nTask: {}\n\nConversation History:\n{}\n\nNext step:",
                prompt,
                project_root,
                instruction,
                conversation_history.join("\n\n")
            )
        };

        let input_tokens = count_tokens(&context);

        // Request info
        println!(
            "{}╔═════════════════════════════════════════════════════════════╗{}",
            YELLOW, RESET
        );
        println!("{}║ {:^59} {}║{}", YELLOW, "📡 REQUEST", YELLOW, RESET);
        println!(
            "{}╠═════════════════════════════════════════════════════════════╣{}",
            YELLOW, RESET
        );
        println!(
            "{}║ {:<15}: {:>6} tokens                            {}║{}",
            YELLOW, "Input Tokens", input_tokens, YELLOW, RESET
        );
        println!(
            "{}╚═════════════════════════════════════════════════════════════╝{}",
            YELLOW, RESET
        );
        println!();

        // Rate limiting
        tpm_limiter.wait_if_needed();
        let current_tpm = tpm_limiter.get_current_tpm();
        let percentage = (current_tpm as f32 / tpm_limit as f32 * 100.0).min(100.0);

        println!(
            "{}╔═════════════════════════════════════════════════════════════╗{}",
            GREEN, RESET
        );
        println!("{}║ {:^59} {}║{}", GREEN, "📊 RATE LIMIT", GREEN, RESET);
        println!(
            "{}╠═════════════════════════════════════════════════════════════╣{}",
            GREEN, RESET
        );
        println!(
            "{}║ {:<15}: {:>6} / {:>6} tokens ({:>5.1}%)      {}║{}",
            GREEN, "Current TPM", current_tpm, tpm_limit, percentage, GREEN, RESET
        );

        let bar_width = 50;
        let filled = (bar_width as f32 * percentage / 100.0) as usize;
        let bar_color = if percentage < 70.0 { GREEN } else { RED };
        println!(
            "{}║ {:<15}: {}{}{:.<width$}{}║{}",
            GREEN,
            "Usage",
            bar_color,
            "█".repeat(filled),
            "",
            RESET,
            width = bar_width - filled
        );

        println!(
            "{}║ {:<15}: {:>6} tokens                            {}║{}",
            GREEN, "Total Tokens", total_tokens, GREEN, RESET
        );
        println!(
            "{}╚═════════════════════════════════════════════════════════════╝{}",
            GREEN, RESET
        );
        println!();

        // LLM Request
        println!(
            "{}╔═════════════════════════════════════════════════════════════╗{}",
            YELLOW, RESET
        );
        println!("{}║ {:^59} {}║{}", YELLOW, "🧠 AI THINKING", YELLOW, RESET);
        println!(
            "{}║ {:^59} {}║{}",
            YELLOW, "Sending request to LLM...", YELLOW, RESET
        );
        println!(
            "{}╚═════════════════════════════════════════════════════════════╝{}",
            YELLOW, RESET
        );
        println!();

        match client.generate(&context, &serde_json::json!({})).await {
            Ok(resp) => {
                let raw_response = resp.to_string();
                let response = filter_thinking_tokens(&raw_response);
                let output_tokens = count_tokens(&response);
                let total_tokens = input_tokens + output_tokens;

                tpm_limiter.add_token_usage(total_tokens);

                // Response stats
                println!(
                    "{}╔═════════════════════════════════════════════════════════════╗{}",
                    BLUE, RESET
                );
                println!("{}║ {:^59} {}║{}", BLUE, "📥 RESPONSE", BLUE, RESET);
                println!(
                    "{}╠═════════════════════════════════════════════════════════════╣{}",
                    BLUE, RESET
                );
                println!(
                    "{}║ {:<15}: {:>6} tokens                            {}║{}",
                    BLUE, "Output Tokens", output_tokens, BLUE, RESET
                );
                println!(
                    "{}║ {:<15}: {:>6} tokens                            {}║{}",
                    BLUE, "Total Tokens", total_tokens, BLUE, RESET
                );
                println!(
                    "{}╚═════════════════════════════════════════════════════════════╝{}",
                    BLUE, RESET
                );
                println!();

                // Display AI response (truncated)
                let display_response: String = response
                    .lines()
                    .take(10)
                    .map(|line| {
                        if line.chars().count() > 75 {
                            format!("{}...", &line[..72])
                        } else {
                            line.to_string()
                        }
                    })
                    .collect::<Vec<String>>()
                    .join("\n");

                println!(
                    "{}╔═════════════════════════════════════════════════════════════╗{}",
                    CYAN, RESET
                );
                println!("{}║ {:^59} {}║{}", CYAN, "💭 AI THOUGHTS", CYAN, RESET);
                println!(
                    "{}╠═════════════════════════════════════════════════════════════╣{}",
                    CYAN, RESET
                );
                for line in display_response.lines() {
                    println!("{}║ {:<57} {}║{}", CYAN, line, CYAN, RESET);
                }
                if response.lines().count() > 10 {
                    println!(
                        "{}║ {:^59} {}║{}",
                        CYAN,
                        format!("... ({} more lines)", response.lines().count() - 10),
                        CYAN,
                        RESET
                    );
                }
                println!(
                    "{}╚═════════════════════════════════════════════════════════════╝{}",
                    CYAN, RESET
                );
                println!();

                conversation_history.push(format!("Assistant: {}", response));

                let tools = extract_tools(&response);

                // Tools info
                println!(
                    "{}╔═════════════════════════════════════════════════════════════╗{}",
                    MAGENTA, RESET
                );
                println!("{}║ {:^59} {}║{}", MAGENTA, "🛠️  TOOLS", MAGENTA, RESET);
                println!(
                    "{}╠═════════════════════════════════════════════════════════════╣{}",
                    MAGENTA, RESET
                );
                println!(
                    "{}║ {:<15}: {:>2} tools                                  {}║{}",
                    MAGENTA,
                    "Tools Extracted",
                    tools.len(),
                    MAGENTA,
                    RESET
                );
                println!(
                    "{}╚═════════════════════════════════════════════════════════════╝{}",
                    MAGENTA, RESET
                );
                println!();

                if tools.is_empty() {
                    println!(
                        "{}╔═════════════════════════════════════════════════════════════╗{}",
                        RED, RESET
                    );
                    println!(
                        "{}║ {:^59} {}║{}",
                        RED, "🔍 DEBUG - NO TOOLS FOUND", RED, RESET
                    );
                    println!(
                        "{}╠═════════════════════════════════════════════════════════════╣{}",
                        RED, RESET
                    );
                    println!(
                        "{}║ {:<20}: {:<36} {}║{}",
                        RED,
                        "read_file found",
                        response.contains("read_file(").to_string(),
                        RED,
                        RESET
                    );
                    println!(
                        "{}║ {:<20}: {:<36} {}║{}",
                        RED,
                        "execute_command found",
                        response.contains("execute_command").to_string(),
                        RED,
                        RESET
                    );
                    println!(
                        "{}║ {:<20}: {:<36} {}║{}",
                        RED,
                        "CHANGE: found",
                        response.contains("CHANGE:").to_string(),
                        RED,
                        RESET
                    );
                    println!(
                        "{}╚═════════════════════════════════════════════════════════════╝{}",
                        RED, RESET
                    );
                    println!();

                    print!("{}Continue? (y/n): {} ", YELLOW, RESET);
                    io::stdout().flush().ok();
                    let mut input = String::new();
                    io::stdin().read_line(&mut input).unwrap();
                    if input.trim().to_lowercase() != "y" {
                        break;
                    }
                    continue;
                }

                let mut all_results = Vec::new();
                let mut success_achieved = false;

                for (i, (tool, param)) in tools.iter().enumerate() {
                    ui.clear_screen();
                    ui.draw_header();
                    ui.draw_status_bar(iteration, total_tokens, current_tpm, tpm_limit);
                    println!();

                    // Tool execution header
                    println!(
                        "{}╔═════════════════════════════════════════════════════════════╗{}",
                        GREEN, RESET
                    );
                    println!(
                        "{}║ {:^59} {}║{}",
                        GREEN,
                        &format!("⚡ EXECUTING TOOL {}/{}", i + 1, tools.len()),
                        GREEN,
                        RESET
                    );
                    println!(
                        "{}╠═════════════════════════════════════════════════════════════╣{}",
                        GREEN, RESET
                    );
                    println!(
                        "{}║ {:<10}: {:<47} {}║{}",
                        GREEN, "Tool", tool, GREEN, RESET
                    );

                    let display_param = if param.chars().count() > 47 {
                        format!("{}...", &param[..44])
                    } else {
                        param.to_string()
                    };
                    println!(
                        "{}║ {:<10}: {:<47} {}║{}",
                        GREEN, "Param", display_param, GREEN, RESET
                    );
                    println!(
                        "{}╚═════════════════════════════════════════════════════════════╝{}",
                        GREEN, RESET
                    );
                    println!();

                    let result = execute_tool(tool, param, &project_root);

                    // Display result
                    println!(
                        "{}╔═════════════════════════════════════════════════════════════╗{}",
                        BLUE, RESET
                    );
                    println!("{}║ {:^59} {}║{}", BLUE, "📋 RESULT", BLUE, RESET);
                    println!(
                        "{}╠═════════════════════════════════════════════════════════════╣{}",
                        BLUE, RESET
                    );

                    for (_j, line) in result.lines().take(15).enumerate() {
                        let display_line = if line.chars().count() > 57 {
                            format!("{}...", &line[..54])
                        } else {
                            line.to_string()
                        };

                        let line_color = if line.contains("Error") || line.contains("error:") {
                            RED
                        } else if line.contains("exit_code: 0") || line.contains("Applied delta") {
                            GREEN
                        } else {
                            CYAN
                        };

                        println!(
                            "{}║ {}{:<57}{}║{}",
                            BLUE, line_color, display_line, BLUE, RESET
                        );
                    }

                    if result.lines().count() > 15 {
                        println!(
                            "{}║ {:^59} {}║{}",
                            BLUE,
                            format!("... ({} more lines)", result.lines().count() - 15),
                            BLUE,
                            RESET
                        );
                    }
                    println!(
                        "{}╚═════════════════════════════════════════════════════════════╝{}",
                        BLUE, RESET
                    );
                    println!();

                    all_results.push(format!("{}: {}", tool, result));

                    // Check for success condition
                    if tool == "execute_command" && param.contains("cargo run") {
                        if result.contains("exit_code: 0")
                            && !result.to_lowercase().contains("error")
                        {
                            success_achieved = true;
                        }
                    }

                    thread::sleep(Duration::from_secs(1));
                }

                conversation_history.push(format!("Tool Results:\n{}", all_results.join("\n")));

                if conversation_history.len() > 10 {
                    conversation_history.drain(0..conversation_history.len() - 10);
                }

                if success_achieved {
                    ui.clear_screen();
                    ui.draw_header();
                    println!();
                    println!(
                        "{}╔═════════════════════════════════════════════════════════════╗{}",
                        BG_GREEN, RESET
                    );
                    println!("{}║ {:^59} {}║{}", BG_GREEN, "🎉 SUCCESS!", BG_GREEN, RESET);
                    println!(
                        "{}║ {:^59} {}║{}",
                        BG_GREEN, "✓ TASK COMPLETED!", BG_GREEN, RESET
                    );
                    println!(
                        "{}║ {:^59} {}║{}",
                        BG_GREEN, "All objectives achieved!", BG_GREEN, RESET
                    );
                    println!(
                        "{}╚═════════════════════════════════════════════════════════════╝{}",
                        BG_GREEN, RESET
                    );
                    return;
                }

                thread::sleep(Duration::from_secs(2));
            }
            Err(err) => {
                println!(
                    "{}╔═════════════════════════════════════════════════════════════╗{}",
                    RED, RESET
                );
                println!("{}║ {:^59} {}║{}", RED, "💥 ERROR", RED, RESET);
                println!(
                    "{}╠═════════════════════════════════════════════════════════════╣{}",
                    RED, RESET
                );

                let error_msg = err.to_string();
                let display_error = if error_msg.chars().count() > 57 {
                    format!("{}...", &error_msg[..54])
                } else {
                    error_msg
                };

                println!("{}║ {:<57} {}║{}", RED, display_error, RED, RESET);
                println!(
                    "{}╚═════════════════════════════════════════════════════════════╝{}",
                    RED, RESET
                );
                println!();

                print!("{}Continue? (y/n): {} ", YELLOW, RESET);
                io::stdout().flush().ok();
                let mut input = String::new();
                io::stdin().read_line(&mut input).unwrap();
                if input.trim().to_lowercase() != "y" {
                    break;
                }
            }
        }
    }
}

fn extract_tools(text: &str) -> Vec<(String, String)> {
    let mut tools = Vec::new();

    // Clean the text first
    let cleaned_text = text
        .replace("```rust", "")
        .replace("```sh", "")
        .replace("```bash", "")
        .replace("```", "");
    let text = cleaned_text.as_str();

    // Extract read_file calls
    if text.contains("read_file") {
        for line in text.lines() {
            if line.contains("read_file(") {
                if let Some(start) = line.find("read_file(") {
                    let after_open = &line[start + 10..];
                    if let Some(end) = after_open.find(')') {
                        let param = after_open[..end]
                            .trim()
                            .trim_matches('"')
                            .trim_matches('\'')
                            .to_string();
                        if !param.is_empty() {
                            tools.push(("read_file".to_string(), param));
                        }
                    }
                }
            }
        }
    }

    // Extract execute_command calls
    if text.contains("execute_command") {
        for line in text.lines() {
            if line.contains("execute_command") {
                // Handle execute_command("command") format
                if let Some(start) = line.find("execute_command(") {
                    let after_open = &line[start + 15..];
                    if let Some(end) = after_open.find(')') {
                        let content = &after_open[..end];
                        // Extract content between quotes
                        if let Some(quote_start) = content.find('"') {
                            if let Some(quote_end) = content[quote_start + 1..].find('"') {
                                let param = &content[quote_start + 1..quote_start + 1 + quote_end];
                                if !param.is_empty() {
                                    tools.push(("execute_command".to_string(), param.to_string()));
                                }
                            }
                        }
                    }
                }
                // Handle execute_command: "command" format
                else if let Some(start) = line.find("execute_command:") {
                    let after_colon = line[start + 15..].trim();
                    // Extract quoted content
                    if after_colon.starts_with('"') {
                        if let Some(end_quote) = after_colon[1..].find('"') {
                            let param = &after_colon[1..1 + end_quote];
                            if !param.is_empty() {
                                tools.push(("execute_command".to_string(), param.to_string()));
                            }
                        }
                    }
                }
            }
        }
    }

    // Extract file changes - FIXED DELTA PARSING
    if text.contains("CHANGE:") {
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            if line.starts_with("CHANGE:") {
                let file_path = line.replace("CHANGE:", "").trim().to_string();

                // Look for the delta pattern
                let mut current_content = String::new();
                let mut new_content = String::new();
                let mut in_current = false;
                let mut in_new = false;

                i += 1;
                while i < lines.len() {
                    let current_line = lines[i].trim();

                    if current_line.contains("<<<<<<< CURRENT") {
                        in_current = true;
                        in_new = false;
                    } else if current_line.contains("=======") {
                        in_current = false;
                        in_new = true;
                    } else if current_line.contains(">>>>>>> NEW") {
                        break;
                    } else if in_current {
                        current_content.push_str(current_line);
                        current_content.push('\n');
                    } else if in_new {
                        new_content.push_str(current_line);
                        new_content.push('\n');
                    }

                    i += 1;
                }

                if !file_path.is_empty() && (!new_content.is_empty() || current_content.is_empty())
                {
                    tools.push((
                        "write_file_delta".to_string(),
                        format!(
                            "{}:::{}\n{}",
                            file_path,
                            current_content.trim(),
                            new_content.trim()
                        ),
                    ));
                }
            }

            i += 1;
        }
    }

    // Remove duplicates while preserving order
    let mut unique_tools = Vec::new();
    for tool in tools {
        if !unique_tools.contains(&tool) {
            unique_tools.push(tool);
        }
    }

    unique_tools
}

fn execute_tool(tool: &str, param: &str, root: &str) -> String {
    match tool {
        "read_file" => {
            let path = Path::new(root).join(param);
            fs::read_to_string(&path).unwrap_or_else(|e| format!("Error: {}", e))
        }
        "write_file_delta" => {
            let parts: Vec<&str> = param.splitn(2, ":::").collect();
            if parts.len() == 2 {
                let path = Path::new(root).join(parts[0].trim());
                let content_parts: Vec<&str> = parts[1].splitn(2, '\n').collect();

                if content_parts.len() == 2 {
                    let old_content = content_parts[0].trim();
                    let new_content = content_parts[1].trim();

                    apply_delta(&path, old_content, new_content)
                } else {
                    "Error: Invalid delta format - missing content separator".to_string()
                }
            } else {
                "Error: Invalid write_file_delta format - missing path separator".to_string()
            }
        }
        "execute_command" => {
            let output = if cfg!(target_os = "windows") {
                Command::new("cmd")
                    .args(["/C", param])
                    .current_dir(root)
                    .output()
            } else {
                Command::new("sh")
                    .arg("-c")
                    .arg(param)
                    .current_dir(root)
                    .output()
            };

            match output {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let exit_code = output.status.code().unwrap_or(-1);

                    format!(
                        "stdout:\n{}\nstderr:\n{}\nexit_code: {}",
                        stdout, stderr, exit_code
                    )
                }
                Err(e) => {
                    format!("Error executing command: {}", e)
                }
            }
        }
        _ => format!("Unknown tool: {}", tool),
    }
}

fn apply_delta(path: &Path, old_content: &str, new_content: &str) -> String {
    // Read existing file content
    let existing_content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => {
            // File doesn't exist, create it with new content
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            return match fs::write(path, new_content) {
                Ok(_) => format!("Created new file: {}", path.display()),
                Err(e) => format!("Error creating file: {}", e),
            };
        }
    };

    // If old_content is empty, replace entire file
    if old_content.is_empty() {
        return match fs::write(path, new_content) {
            Ok(_) => format!("Replaced entire file: {}", path.display()),
            Err(e) => format!("Error replacing file: {}", e),
        };
    }

    // Find and replace the specific content
    if let Some(pos) = existing_content.find(old_content) {
        let mut updated_content = String::new();
        updated_content.push_str(&existing_content[..pos]);
        updated_content.push_str(new_content);
        updated_content.push_str(&existing_content[pos + old_content.len()..]);

        match fs::write(path, updated_content) {
            Ok(_) => format!("Successfully applied delta to: {}", path.display()),
            Err(e) => format!("Error applying delta: {}", e),
        }
    } else {
        format!(
            "Error: Could not find the specified content in {}\nLooking for:\n{}\n\nCurrent file content:\n{}",
            path.display(),
            old_content,
            existing_content
        )
    }
}
