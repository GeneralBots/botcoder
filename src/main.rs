use dotenv::dotenv;
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

struct TPMLimiter {
    max_tpm: u32,
    min_interval: Duration,
    token_usage: VecDeque<(SystemTime, u32)>, // (timestamp, tokens)
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

    fn print_progress_bar(&self, percentage: f32, elapsed: Duration, total: Duration) {
        let bar_width = 30;
        let filled = (bar_width as f32 * percentage / 100.0) as usize;
        let empty = bar_width - filled;

        print!(
            "\r│ [{}{}] {:>5.1}% │ {:>4.1}s / {:>4.1}s remaining │",
            "█".repeat(filled),
            "░".repeat(empty),
            percentage,
            elapsed.as_secs_f32(),
            total.as_secs_f32()
        );
        io::stdout().flush().ok();
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

    fn print_tpm_status(&self) {
        let current_tpm = self.get_current_tpm();
        let percentage = (current_tpm as f32 / self.max_tpm as f32 * 100.0).min(100.0);
        let bar_width = 30;
        let filled = (bar_width as f32 * percentage / 100.0) as usize;

        println!("┌─────────────────────────────────────────────────────────────┐");
        println!("│ TPM STATUS                                                  │");
        println!("├─────────────────────────────────────────────────────────────┤");
        println!(
            "│ Current Usage: {:>6} / {:>6} tokens ({:>5.1}%)            │",
            current_tpm, self.max_tpm, percentage
        );
        println!(
            "│ [{}{}] │",
            "█".repeat(filled),
            "░".repeat(bar_width - filled)
        );
        println!(
            "│ Total Tokens:  {:>6} tokens                               │",
            self.total_tokens_used
        );
        println!("└─────────────────────────────────────────────────────────────┘");
    }

    fn add_token_usage(&mut self, tokens: u32) {
        let now = SystemTime::now();
        self.token_usage.push_back((now, tokens));
        self.total_tokens_used += tokens;

        // Clean old entries
        let one_minute_ago = now - Duration::from_secs(60);
        while let Some((time, _)) = self.token_usage.front() {
            if *time < one_minute_ago {
                self.token_usage.pop_front();
            } else {
                break;
            }
        }
    }

    fn wait_if_needed(&mut self) {
        let now = SystemTime::now();

        // First, enforce minimum interval between requests
        if let Some(last_req) = self.last_request {
            if let Ok(elapsed) = last_req.elapsed() {
                if elapsed < self.min_interval {
                    let wait_time = self.min_interval - elapsed;
                    println!("┌─────────────────────────────────────────────────────────────┐");
                    println!(
                        "│ RATE LIMIT: Minimum {} second interval                     │",
                        self.min_interval.as_secs()
                    );
                    println!("└─────────────────────────────────────────────────────────────┘");

                    // Progress bar for minimum interval wait
                    let start = SystemTime::now();
                    while start.elapsed().unwrap() < wait_time {
                        let elapsed = start.elapsed().unwrap();
                        let percentage = (elapsed.as_secs_f32() / wait_time.as_secs_f32()) * 100.0;
                        let remaining = wait_time - elapsed;
                        self.print_progress_bar(percentage, elapsed, remaining);
                        thread::sleep(Duration::from_millis(100));
                    }
                    self.print_progress_bar(100.0, wait_time, Duration::from_secs(0));
                    println!();
                }
            }
        }

        // Check TPM limit
        let current_tpm = self.get_current_tpm();

        // If we're at the limit, wait
        if current_tpm >= self.max_tpm {
            if let Some((oldest_time, _)) = self.token_usage.front() {
                if let Ok(elapsed) = oldest_time.elapsed() {
                    if elapsed < Duration::from_secs(60) {
                        let wait_time =
                            Duration::from_secs(60) - elapsed + Duration::from_millis(100);
                        println!("┌─────────────────────────────────────────────────────────────┐");
                        println!(
                            "│ TPM LIMIT REACHED: {}/{} tokens                          │",
                            current_tpm, self.max_tpm
                        );
                        println!("│ Waiting for token window to refresh...                     │");
                        println!("└─────────────────────────────────────────────────────────────┘");

                        let start = SystemTime::now();
                        while start.elapsed().unwrap() < wait_time {
                            let elapsed = start.elapsed().unwrap();
                            let percentage =
                                (elapsed.as_secs_f32() / wait_time.as_secs_f32()) * 100.0;
                            let remaining = wait_time - elapsed;
                            self.print_progress_bar(percentage, elapsed, remaining);
                            thread::sleep(Duration::from_millis(100));
                        }
                        self.print_progress_bar(100.0, wait_time, Duration::from_secs(0));
                        println!();
                    }
                }
            }
        }

        self.last_request = Some(now);
    }
}

fn count_tokens(text: &str) -> u32 {
    // Simple approximation: ~4 chars per token
    (text.len() / 4) as u32
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

    // Clear screen and print header
    print!("\x1B[2J\x1B[1;1H");
    println!("╔═════════════════════════════════════════════════════════════╗");
    println!("║          RUST CODE AGENT - LLM Powered Automation          ║");
    println!("╚═════════════════════════════════════════════════════════════╝");
    println!();

    let instruction =
        env::var("INSTRUCTION").expect("Please set the INSTRUCTION variable in your .env file");

    let client = match AzureOpenAIClient::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("┌─────────────────────────────────────────────────────────────┐");
            eprintln!("│ ERROR: Failed to create AzureOpenAIClient                   │");
            eprintln!(
                "│ {}                                                          │",
                e
            );
            eprintln!("└─────────────────────────────────────────────────────────────┘");
            return;
        }
    };

    let prompt = fs::read_to_string("prompt.txt").expect("Failed to read prompt.txt");
    let project_root = env::var("PROJECT_PATH").expect("PROJECT_PATH not set");

    // Initialize TPM limiter with 10 second minimum interval
    let tpm_limit: u32 = env::var("LLM_TPM")
        .unwrap_or_else(|_| "20000".to_string())
        .parse()
        .unwrap_or(20000);

    let min_interval_secs: u64 = env::var("LLM_MIN_INTERVAL")
        .unwrap_or_else(|_| "10".to_string())
        .parse()
        .unwrap_or(10);

    let mut tpm_limiter = TPMLimiter::new(tpm_limit, min_interval_secs);

    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ CONFIGURATION                                               │");
    println!("├─────────────────────────────────────────────────────────────┤");
    println!(
        "│ Project Path:    {}                                         │",
        format!("{:<40}", project_root)
            .chars()
            .take(40)
            .collect::<String>()
    );
    println!(
        "│ Max TPM:         {:>6} tokens/minute                       │",
        tpm_limit
    );
    println!(
        "│ Min Interval:    {:>6} seconds                             │",
        min_interval_secs
    );
    println!("└─────────────────────────────────────────────────────────────┘");
    println!();

    let mut iteration = 0;
    let mut conversation_history: Vec<String> = Vec::new();

    loop {
        iteration += 1;

        println!("╔═════════════════════════════════════════════════════════════╗");
        println!(
            "║ ITERATION {:>2}                                               ║",
            iteration
        );
        println!("╚═════════════════════════════════════════════════════════════╝");
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

        // Count input tokens
        let input_tokens = count_tokens(&context);

        println!("┌─────────────────────────────────────────────────────────────┐");
        println!("│ REQUEST                                                     │");
        println!("├─────────────────────────────────────────────────────────────┤");
        println!(
            "│ Input Tokens:  {:>6} tokens                               │",
            input_tokens
        );
        println!("└─────────────────────────────────────────────────────────────┘");
        println!();

        // Apply TPM rate limiting before making LLM request
        tpm_limiter.wait_if_needed();
        tpm_limiter.print_tpm_status();
        println!();

        println!("┌─────────────────────────────────────────────────────────────┐");
        println!("│ Sending request to LLM...                                   │");
        println!("└─────────────────────────────────────────────────────────────┘");

        match client.generate(&context, &serde_json::json!({})).await {
            Ok(resp) => {
                let raw_response = resp.to_string();
                let response = filter_thinking_tokens(&raw_response);
                let output_tokens = count_tokens(&response);
                let total_tokens = input_tokens + output_tokens;

                // Add token usage
                tpm_limiter.add_token_usage(total_tokens);

                println!();
                println!("┌─────────────────────────────────────────────────────────────┐");
                println!("│ RESPONSE                                                    │");
                println!("├─────────────────────────────────────────────────────────────┤");
                println!(
                    "│ Output Tokens: {:>6} tokens                               │",
                    output_tokens
                );
                println!(
                    "│ Total Tokens:  {:>6} tokens                               │",
                    total_tokens
                );
                println!("└─────────────────────────────────────────────────────────────┘");
                println!();

                // Print response in a box
                println!("┌─────────────────────────────────────────────────────────────┐");
                println!("│ AI RESPONSE:                                                │");
                println!("└─────────────────────────────────────────────────────────────┘");
                for line in response.lines().take(20) {
                    let trimmed = if line.len() > 61 {
                        format!("{}...", &line[..58])
                    } else {
                        line.to_string()
                    };
                    println!("  {}", trimmed);
                }
                if response.lines().count() > 20 {
                    println!("  ... ({} more lines)", response.lines().count() - 20);
                }
                println!();

                // DEBUG: Print raw response for analysis
                println!("┌─────────────────────────────────────────────────────────────┐");
                println!("│ DEBUG: RAW RESPONSE ANALYSIS                               │");
                println!("├─────────────────────────────────────────────────────────────┤");
                println!(
                    "│ Raw response length: {:>6} characters               │",
                    raw_response.len()
                );
                println!(
                    "│ Filtered length:     {:>6} characters               │",
                    response.len()
                );
                println!(
                    "│ Contains 'read_file': {}                             │",
                    response.contains("read_file")
                );
                println!(
                    "│ Contains 'execute_command': {}                       │",
                    response.contains("execute_command")
                );
                println!(
                    "│ Contains 'CHANGE:': {}                               │",
                    response.contains("CHANGE:")
                );
                println!("└─────────────────────────────────────────────────────────────┘");
                println!();

                conversation_history.push(format!("Assistant: {}", response));

                let tools = extract_tools(&response);

                println!("┌─────────────────────────────────────────────────────────────┐");
                println!(
                    "│ TOOLS EXTRACTED: {:>2}                                       │",
                    tools.len()
                );
                println!("└─────────────────────────────────────────────────────────────┘");

                // DEBUG: Show what was found
                if tools.is_empty() {
                    println!();
                    println!("┌─────────────────────────────────────────────────────────────┐");
                    println!("│ DEBUG: TOOL EXTRACTION FAILED                              │");
                    println!("├─────────────────────────────────────────────────────────────┤");
                    println!("│ Searching for patterns in response...                      │");

                    // Check for common patterns that might indicate tools
                    let lines: Vec<&str> = response.lines().collect();
                    for (i, line) in lines.iter().enumerate().take(10) {
                        println!(
                            "│ Line {}: {:>60} │",
                            i,
                            if line.len() > 60 {
                                format!("{}...", &line[..57])
                            } else {
                                line.to_string()
                            }
                        );
                    }

                    // Check for specific patterns
                    println!("│ PATTERN ANALYSIS:                                        │");
                    println!(
                        "│ - 'read_file(' found: {}                                │",
                        response.contains("read_file(")
                    );
                    println!(
                        "│ - 'execute_command(' found: {}                          │",
                        response.contains("execute_command(")
                    );
                    println!(
                        "│ - 'execute_command:' found: {}                          │",
                        response.contains("execute_command:")
                    );
                    println!(
                        "│ - 'CHANGE:' found: {}                                    │",
                        response.contains("CHANGE:")
                    );
                    println!(
                        "│ - '<<<<<<<' found: {}                                    │",
                        response.contains("<<<<<<<")
                    );
                    println!(
                        "│ - '>>>>>>>' found: {}                                    │",
                        response.contains(">>>>>>>")
                    );
                    println!("└─────────────────────────────────────────────────────────────┘");
                    println!();

                    println!("┌─────────────────────────────────────────────────────────────┐");
                    println!("│ WARNING: No tools found                                     │");
                    println!("│ Continue? (y/n):                                            │");
                    println!("└─────────────────────────────────────────────────────────────┘");
                    print!("> ");
                    io::stdout().flush().ok();
                    let mut input = String::new();
                    io::stdin().read_line(&mut input).unwrap();
                    if input.trim() != "y" {
                        break;
                    }
                    continue;
                } else {
                    println!();
                    println!("┌─────────────────────────────────────────────────────────────┐");
                    println!("│ DEBUG: TOOLS FOUND                                         │");
                    println!("├─────────────────────────────────────────────────────────────┤");
                    for (i, (tool, param)) in tools.iter().enumerate() {
                        println!(
                            "│ Tool {}: {:<20} -> {:>30} │",
                            i + 1,
                            tool,
                            if param.len() > 30 {
                                format!("{}...", &param[..27])
                            } else {
                                param.to_string()
                            }
                        );
                    }
                    println!("└─────────────────────────────────────────────────────────────┘");
                    println!();
                }

                let mut all_results = Vec::new();

                for (i, (tool, param)) in tools.iter().enumerate() {
                    println!();
                    println!("┌─────────────────────────────────────────────────────────────┐");
                    println!(
                        "│ EXECUTING TOOL {}/{}                                         │",
                        i + 1,
                        tools.len()
                    );
                    println!("├─────────────────────────────────────────────────────────────┤");
                    println!(
                        "│ Tool:  {}                                                   │",
                        format!("{:<50}", tool).chars().take(50).collect::<String>()
                    );
                    println!(
                        "│ Param: {}                                                   │",
                        format!(
                            "{:<50}",
                            if param.len() > 50 {
                                format!("{}...", &param[..47])
                            } else {
                                param.to_string()
                            }
                        )
                        .chars()
                        .take(50)
                        .collect::<String>()
                    );
                    println!("└─────────────────────────────────────────────────────────────┘");

                    let result = execute_tool(tool, param, &project_root);

                    println!();
                    println!("┌─────────────────────────────────────────────────────────────┐");
                    println!("│ RESULT:                                                     │");
                    println!("└─────────────────────────────────────────────────────────────┘");
                    for line in result.lines().take(15) {
                        let trimmed = if line.len() > 61 {
                            format!("{}...", &line[..58])
                        } else {
                            line.to_string()
                        };
                        println!("  {}", trimmed);
                    }
                    if result.lines().count() > 15 {
                        println!("  ... ({} more lines)", result.lines().count() - 15);
                    }

                    all_results.push(format!("Tool: {}\nResult:\n{}", tool, result));

                    if tool == "execute_command" && param.contains("cargo run") {
                        if result.contains("exit_code: 0")
                            && !result.contains("error:")
                            && !result.contains("Error")
                        {
                            println!();
                            println!(
                                "╔═════════════════════════════════════════════════════════════╗"
                            );
                            println!(
                                "║                    ✓ SUCCESS                               ║"
                            );
                            println!(
                                "║              Task Completed Successfully!                  ║"
                            );
                            println!(
                                "╚═════════════════════════════════════════════════════════════╝"
                            );
                            return;
                        }
                    }
                }

                conversation_history.push(format!("Tool Results:\n{}", all_results.join("\n\n")));

                if conversation_history.len() > 20 {
                    conversation_history.drain(0..10);
                }

                println!();
                tpm_limiter.print_tpm_status();
                println!();
            }
            Err(err) => {
                let e = err.to_string();
                println!();
                println!("┌─────────────────────────────────────────────────────────────┐");
                println!("│ ERROR                                                       │");
                println!("├─────────────────────────────────────────────────────────────┤");
                println!(
                    "│ {}                                                          │",
                    format!(
                        "{:<55}",
                        if e.len() > 55 {
                            format!("{}...", &e[..52])
                        } else {
                            e
                        }
                    )
                );
                println!("│                                                             │");
                println!("│ Continue? (y/n):                                            │");
                println!("└─────────────────────────────────────────────────────────────┘");
                print!("> ");
                io::stdout().flush().ok();
                let mut input = String::new();
                io::stdin().read_line(&mut input).unwrap();
                if input.trim() != "y" {
                    break;
                }
            }
        }
    }
}

fn extract_tools(text: &str) -> Vec<(String, String)> {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ DEBUG: EXTRACT_TOOLS CALLED                                 │");
    println!("├─────────────────────────────────────────────────────────────┤");

    // Strip code blocks first to handle markdown formatting
    let cleaned_text = text
        .replace("```rust", "")
        .replace("```sh", "")
        .replace("```bash", "")
        .replace("```", "");
    let text = cleaned_text.as_str();

    println!(
        "│ Text after cleaning: {:>6} chars                    │",
        text.len()
    );
    println!("└─────────────────────────────────────────────────────────────┘");

    let mut tools = Vec::new();

    // Extract read_file calls
    if text.contains("read_file") {
        println!("┌─────────────────────────────────────────────────────────────┐");
        println!("│ DEBUG: Searching for read_file patterns                     │");
        for line in text.lines() {
            if line.contains("read_file") {
                println!(
                    "│ Found 'read_file' in line: {:>40} │",
                    if line.len() > 40 {
                        format!("{}...", &line[..37])
                    } else {
                        line.to_string()
                    }
                );

                if let Some(start) = line.find("read_file(") {
                    if let Some(end) = line[start..].find(')') {
                        let param = line[start + 10..start + end]
                            .trim()
                            .trim_matches('"')
                            .trim_matches('\'')
                            .to_string();
                        if !param.is_empty() {
                            println!(
                                "│ Extracted read_file param: {:>30} │",
                                if param.len() > 30 {
                                    format!("{}...", &param[..27])
                                } else {
                                    param.to_string()
                                }
                            );
                            tools.push(("read_file".to_string(), param));
                        }
                    }
                }
            }
        }
        println!("└─────────────────────────────────────────────────────────────┘");
    }

    // Extract execute_command calls - more flexible approach
    if text.contains("execute_command") {
        println!("┌─────────────────────────────────────────────────────────────┐");
        println!("│ DEBUG: Searching for execute_command patterns               │");

        for line in text.lines() {
            if line.contains("execute_command") {
                println!(
                    "│ Found 'execute_command' in line: {:>35} │",
                    if line.len() > 35 {
                        format!("{}...", &line[..32])
                    } else {
                        line.to_string()
                    }
                );

                // Handle execute_command("command") format
                if let Some(start) = line.find("execute_command(") {
                    let after_open = &line[start + 15..]; // After "execute_command("
                    if let Some(end) = after_open.find(')') {
                        let content = &after_open[..end];
                        // Extract content between quotes
                        if let Some(quote_start) = content.find('"') {
                            if let Some(quote_end) = content[quote_start + 1..].find('"') {
                                let param = &content[quote_start + 1..quote_start + 1 + quote_end];
                                if !param.is_empty() {
                                    println!(
                                        "│ Extracted execute_command param: {:>25} │",
                                        if param.len() > 25 {
                                            format!("{}...", &param[..22])
                                        } else {
                                            param.to_string()
                                        }
                                    );
                                    tools.push(("execute_command".to_string(), param.to_string()));
                                }
                            }
                        } else if let Some(quote_start) = content.find('\'') {
                            if let Some(quote_end) = content[quote_start + 1..].find('\'') {
                                let param = &content[quote_start + 1..quote_start + 1 + quote_end];
                                if !param.is_empty() {
                                    println!(
                                        "│ Extracted execute_command param: {:>25} │",
                                        if param.len() > 25 {
                                            format!("{}...", &param[..22])
                                        } else {
                                            param.to_string()
                                        }
                                    );
                                    tools.push(("execute_command".to_string(), param.to_string()));
                                }
                            }
                        }
                    }
                }
                // Handle execute_command: "command" format
                else if let Some(start) = line.find("execute_command:") {
                    let after_colon = line[start + 15..].trim();
                    println!(
                        "│ After colon: {:>45} │",
                        if after_colon.len() > 45 {
                            format!("{}...", &after_colon[..42])
                        } else {
                            after_colon.to_string()
                        }
                    );

                    // Extract quoted content
                    if after_colon.starts_with('"') {
                        if let Some(end_quote) = after_colon[1..].find('"') {
                            let param = &after_colon[1..1 + end_quote];
                            if !param.is_empty() {
                                println!("│ Extracted quoted param: {:>30} │", param);
                                tools.push(("execute_command".to_string(), param.to_string()));
                            }
                        }
                    } else if after_colon.starts_with('\'') {
                        if let Some(end_quote) = after_colon[1..].find('\'') {
                            let param = &after_colon[1..1 + end_quote];
                            if !param.is_empty() {
                                println!("│ Extracted quoted param: {:>30} │", param);
                                tools.push(("execute_command".to_string(), param.to_string()));
                            }
                        }
                    } else {
                        // No quotes, take until end of line or special characters
                        let param = after_colon
                            .split(|c: char| c == ')' || c == ']' || c == '`' || c == '\n')
                            .next()
                            .unwrap_or(after_colon)
                            .trim()
                            .to_string();
                        if !param.is_empty() {
                            println!("│ Extracted unquoted param: {:>28} │", param);
                            tools.push(("execute_command".to_string(), param));
                        }
                    }
                }
            }
        }
        println!("└─────────────────────────────────────────────────────────────┘");
    }

    // Extract file changes using delta format
    if text.contains("CHANGE:") {
        println!("┌─────────────────────────────────────────────────────────────┐");
        println!("│ DEBUG: Searching for CHANGE patterns                       │");

        let mut current_file = String::new();
        let mut in_change = false;
        let mut in_current = false;
        let mut in_new = false;
        let mut current_content = String::new();
        let mut new_content = String::new();

        for line in text.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("CHANGE:") {
                println!(
                    "│ Found CHANGE: {:>45} │",
                    if trimmed.len() > 45 {
                        format!("{}...", &trimmed[..42])
                    } else {
                        trimmed.to_string()
                    }
                );

                // Save previous change if exists
                if !current_file.is_empty()
                    && (!new_content.is_empty() || current_content.is_empty())
                {
                    tools.push((
                        "write_file_delta".to_string(),
                        format!(
                            "{}:::{}\n{}",
                            current_file,
                            current_content.trim(),
                            new_content.trim()
                        ),
                    ));
                    println!("│ Saved change for: {:>38} │", current_file);
                }

                current_file = trimmed.replace("CHANGE:", "").trim().to_string();
                in_change = true;
                current_content.clear();
                new_content.clear();
                in_current = false;
                in_new = false;
                println!("│ New file: {:>45} │", current_file);
            } else if in_change && trimmed.contains("<<<<<<< CURRENT") {
                in_current = true;
                in_new = false;
                println!("│ Started CURRENT section                           │");
            } else if in_change && trimmed.contains("=======") {
                in_current = false;
                in_new = true;
                println!("│ Started NEW section                               │");
            } else if in_change && trimmed.contains(">>>>>>> NEW") {
                in_new = false;
                in_change = false;
                println!("│ Ended CHANGE section                              │");

                if !current_file.is_empty()
                    && (!new_content.is_empty() || current_content.is_empty())
                {
                    tools.push((
                        "write_file_delta".to_string(),
                        format!(
                            "{}:::{}\n{}",
                            current_file,
                            current_content.trim(),
                            new_content.trim()
                        ),
                    ));
                    println!("│ Saved change for: {:>38} │", current_file);
                }

                current_file.clear();
                current_content.clear();
                new_content.clear();
            } else if in_current {
                current_content.push_str(line);
                current_content.push('\n');
            } else if in_new {
                new_content.push_str(line);
                new_content.push('\n');
            }
        }

        // Handle last change if not closed properly
        if !current_file.is_empty() && (!new_content.is_empty() || current_content.is_empty()) {
            tools.push((
                "write_file_delta".to_string(),
                format!(
                    "{}:::{}\n{}",
                    current_file,
                    current_content.trim(),
                    new_content.trim()
                ),
            ));
            println!("│ Saved final change for: {:>35} │", current_file);
        }
        println!("└─────────────────────────────────────────────────────────────┘");
    }

    // Remove duplicates while preserving order
    let mut unique_tools = Vec::new();
    for tool in tools {
        if !unique_tools.contains(&tool) {
            unique_tools.push(tool);
        }
    }

    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ DEBUG: EXTRACT_TOOLS COMPLETE                               │");
    println!(
        "│ Found {} tools                                            │",
        unique_tools.len()
    );
    println!("└─────────────────────────────────────────────────────────────┘");

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
                let path = Path::new(root).join(parts[0]);
                let content_parts: Vec<&str> = parts[1].splitn(2, '\n').collect();

                if content_parts.len() == 2 {
                    let old_content = content_parts[0].trim();
                    let new_content = content_parts[1].trim();

                    apply_delta(&path, old_content, new_content)
                } else {
                    "Error: Invalid delta format".to_string()
                }
            } else {
                "Error: Invalid write_file_delta format".to_string()
            }
        }
        "execute_command" => {
            let output = Command::new("sh")
                .arg("-c")
                .arg(param)
                .current_dir(root)
                .output()
                .unwrap();

            format!(
                "stdout:\n{}\nstderr:\n{}\nexit_code: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
                output.status.code().unwrap_or(-1)
            )
        }
        _ => String::new(),
    }
}

fn apply_delta(path: &Path, old_content: &str, new_content: &str) -> String {
    let existing_content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => {
            // File doesn't exist - create new file
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).ok();
            }
            return match fs::write(path, new_content) {
                Ok(_) => format!("Created new file {}", path.display()),
                Err(e) => format!("Error creating file: {}", e),
            };
        }
    };

    // If old_content is empty, replace entire file
    if old_content.is_empty() {
        return match fs::write(path, new_content) {
            Ok(_) => format!("Replaced entire file {}", path.display()),
            Err(e) => format!("Error writing file: {}", e),
        };
    }

    // Try to find and replace the old content
    if let Some(pos) = existing_content.find(old_content) {
        let mut new_file_content = String::new();
        new_file_content.push_str(&existing_content[..pos]);
        new_file_content.push_str(new_content);
        new_file_content.push_str(&existing_content[pos + old_content.len()..]);

        match fs::write(path, new_file_content) {
            Ok(_) => format!("Applied delta to {}", path.display()),
            Err(e) => format!("Error applying delta: {}", e),
        }
    } else {
        format!(
            "Error: Could not find the specified content in {}.\nLooking for:\n{}\n\nFile may have changed or content doesn't match exactly.",
            path.display(),
            old_content
        )
    }
}
