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

    fn get_current_tpm(&self) -> u32 {
        let now = SystemTime::now();
        let one_minute_ago = now - Duration::from_secs(60);

        self.token_usage
            .iter()
            .filter(|(time, _)| *time >= one_minute_ago)
            .map(|(_, tokens)| tokens)
            .sum()
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
                    println!(
                        "[RATE LIMIT] Minimum {} second interval",
                        self.min_interval.as_secs()
                    );
                    thread::sleep(wait_time);
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
                        println!(
                            "[TPM LIMIT] {}/{} tokens, waiting...",
                            current_tpm, self.max_tpm
                        );
                        thread::sleep(wait_time);
                    }
                }
            }
        }

        self.last_request = Some(now);
    }
}

fn count_tokens(text: &str) -> u32 {
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

    println!("=== RUST CODE AGENT ===");
    println!();

    let instruction = env::var("INSTRUCTION").expect("Please set INSTRUCTION in .env");
    let client = match AzureOpenAIClient::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ERROR: Failed to create AzureOpenAIClient: {}", e);
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
        .unwrap_or_else(|_| "1".to_string())
        .parse()
        .unwrap_or(1);

    let mut tpm_limiter = TPMLimiter::new(tpm_limit, min_interval_secs);

    println!("Configuration:");
    println!("- Project: {}", project_root);
    println!("- Max TPM: {}", tpm_limit);
    println!("- Min Interval: {}s", min_interval_secs);
    println!();

    let mut iteration = 0;
    let mut conversation_history: Vec<String> = Vec::new();

    loop {
        iteration += 1;
        println!("=== ITERATION {} ===", iteration);

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
        println!("[REQUEST] Input tokens: {}", input_tokens);

        tpm_limiter.wait_if_needed();

        println!("[LLM] Sending request...");
        match client.generate(&context, &serde_json::json!({})).await {
            Ok(resp) => {
                let raw_response = resp.to_string();
                let response = filter_thinking_tokens(&raw_response);
                let output_tokens = count_tokens(&response);
                let total_tokens = input_tokens + output_tokens;

                tpm_limiter.add_token_usage(total_tokens);

                println!(
                    "[RESPONSE] Output tokens: {}, Total: {}",
                    output_tokens, total_tokens
                );
                println!("[RAW RESPONSE] {}", raw_response);
                println!("[FILTERED RESPONSE] {}", response);

                conversation_history.push(format!("Assistant: {}", response));

                let tools = extract_tools(&response);
                println!("[TOOLS] Found {} tools", tools.len());

                if tools.is_empty() {
                    println!("[WARNING] No tools found in response!");
                    println!("Response was: {}", response);
                    println!("Continue? (y/n): ");
                    io::stdout().flush().ok();
                    let mut input = String::new();
                    io::stdin().read_line(&mut input).unwrap();
                    if input.trim() != "y" {
                        break;
                    }
                    continue;
                }

                let mut all_results = Vec::new();

                for (i, (tool, param)) in tools.iter().enumerate() {
                    println!(
                        "[EXECUTE] Tool {}/{}: {} -> {}",
                        i + 1,
                        tools.len(),
                        tool,
                        param
                    );

                    let result = execute_tool(tool, param, &project_root);
                    println!("[RESULT] {}", result);

                    all_results.push(format!("Tool: {}\nResult:\n{}", tool, result));

                    if tool == "execute_command" && param.contains("cargo run") {
                        if result.contains("exit_code: 0")
                            && !result.contains("error:")
                            && !result.contains("Error")
                        {
                            println!("[SUCCESS] Task completed!");
                            return;
                        }
                    }
                }

                conversation_history.push(format!("Tool Results:\n{}", all_results.join("\n\n")));

                if conversation_history.len() > 20 {
                    conversation_history.drain(0..10);
                }

                println!();
            }
            Err(err) => {
                eprintln!("[ERROR] LLM request failed: {}", err);
                println!("Continue? (y/n): ");
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
    println!("[DEBUG] === TOOL EXTRACTION START ===");
    println!("[DEBUG] Input text length: {} chars", text.len());

    let cleaned_text = text
        .replace("```rust", "")
        .replace("```sh", "")
        .replace("```bash", "")
        .replace("```", "");
    let text = cleaned_text.as_str();

    let mut tools = Vec::new();

    // First priority: Extract delta format (file changes)
    let delta_tools = extract_delta_format(text);
    tools.extend(delta_tools);

    // If we found delta tools, return them immediately (most common case)
    if !tools.is_empty() {
        println!(
            "[DEBUG] Found {} delta tools, skipping other extraction",
            tools.len()
        );
        return tools;
    }

    // Second: Look for simple tool patterns
    let simple_tools = extract_simple_tools(text);
    tools.extend(simple_tools);

    println!("[DEBUG] === TOOL EXTRACTION COMPLETE ===");
    println!("[DEBUG] Found {} unique tools:", tools.len());
    for (i, (tool, param)) in tools.iter().enumerate() {
        println!(
            "[DEBUG]   {}. {}: '{}'",
            i + 1,
            tool,
            if param.len() > 50 {
                format!("{}...", &param[..47])
            } else {
                param.to_string()
            }
        );
    }

    tools
}
fn extract_delta_format(text: &str) -> Vec<(String, String)> {
    println!("[DELTA] === DELTA EXTRACTION START ===");
    let mut tools = Vec::new();

    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        if line.starts_with("CHANGE:") {
            println!("[DELTA] Found CHANGE at line {}: {}", i, line);

            let file_path = line.replace("CHANGE:", "").trim().to_string();
            let mut current_content = String::new();
            let mut new_content = String::new();

            i += 1; // Move to next line

            // Look for <<<<<<< CURRENT
            while i < lines.len() && !lines[i].trim().starts_with("<<<<<<< CURRENT") {
                i += 1;
            }

            if i >= lines.len() {
                println!("[DELTA] ERROR: No <<<<<<< CURRENT found after CHANGE");
                break;
            }

            i += 1; // Skip <<<<<<< CURRENT line

            // Collect CURRENT content until =======
            while i < lines.len() && !lines[i].trim().starts_with("=======") {
                current_content.push_str(lines[i]);
                current_content.push_str("\n");
                i += 1;
            }

            if i >= lines.len() {
                println!("[DELTA] ERROR: No ======= found after CURRENT section");
                break;
            }

            i += 1; // Skip ======= line

            // Collect NEW content until >>>>>>> NEW
            while i < lines.len() && !lines[i].trim().starts_with(">>>>>>> NEW") {
                new_content.push_str(lines[i]);
                new_content.push_str("\n");
                i += 1;
            }

            if i >= lines.len() {
                println!("[DELTA] ERROR: No >>>>>>> NEW found after NEW section");
                break;
            }

            i += 1; // Skip >>>>>>> NEW line

            // Trim the content to handle whitespace issues
            let current_trimmed = current_content.trim();
            let new_trimmed = new_content.trim();

            println!("[DELTA] Extracted delta for file: {}", file_path);
            println!(
                "[DELTA] Current content length: {} -> {}",
                current_content.len(),
                current_trimmed.len()
            );
            println!(
                "[DELTA] New content length: {} -> {}",
                new_content.len(),
                new_trimmed.len()
            );

            // Create the tool call
            let tool_param = format!("{}:::{}\n{}", file_path, current_trimmed, new_trimmed);

            tools.push(("write_file_delta".to_string(), tool_param));
        } else {
            i += 1;
        }
    }

    println!("[DELTA] === DELTA EXTRACTION COMPLETE ===");
    println!("[DELTA] Found {} delta changes", tools.len());
    tools
}
fn extract_simple_tools(text: &str) -> Vec<(String, String)> {
    println!("[SIMPLE] === SIMPLE TOOL EXTRACTION START ===");
    let mut tools = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Skip delta format lines
        if line.starts_with("CHANGE:")
            || line.starts_with("<<<<<<<")
            || line.starts_with("=======")
            || line.starts_with(">>>>>>>")
        {
            continue;
        }

        println!("[SIMPLE] Processing line: '{}'", line);

        // read_file patterns
        if line.contains("read_file") {
            // read_file("path")
            if let Some(start) = line.find("read_file(") {
                if let Some(end) = line[start..].find(')') {
                    let param = line[start + 10..start + end]
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string();
                    if !param.is_empty() {
                        println!("[SIMPLE] Found read_file: '{}'", param);
                        tools.push(("read_file".to_string(), param));
                        continue;
                    }
                }
            }
            // read_file: "path"
            if let Some(start) = line.find("read_file:") {
                let after = line[start + 10..].trim();
                if let Some(param) = extract_between_quotes(after) {
                    println!("[SIMPLE] Found read_file: '{}'", param);
                    tools.push(("read_file".to_string(), param));
                    continue;
                }
            }
        }

        // execute_command patterns
        if line.contains("execute_command") {
            // execute_command("cmd")
            if let Some(start) = line.find("execute_command(") {
                let after = line[start + 16..].trim();
                if let Some(end) = after.find(')') {
                    let content = &after[..end];
                    if let Some(param) = extract_between_quotes(content) {
                        println!("[SIMPLE] Found execute_command: '{}'", param);
                        tools.push(("execute_command".to_string(), param));
                        continue;
                    }
                }
            }
            // execute_command: "cmd"
            if let Some(start) = line.find("execute_command:") {
                let after = line[start + 16..].trim();
                if let Some(param) = extract_between_quotes(after) {
                    println!("[SIMPLE] Found execute_command: '{}'", param);
                    tools.push(("execute_command".to_string(), param));
                    continue;
                }
            }
            // OSS GPT weird format
            if line.contains("code{\"command\":\"") {
                if let Some(start) = line.find("code{\"command\":\"") {
                    let after = line[start + 16..].trim();
                    if let Some(end) = after.find('"') {
                        let param = &after[..end];
                        println!("[SIMPLE] Found OSS GPT command: '{}'", param);
                        tools.push(("execute_command".to_string(), param.to_string()));
                        continue;
                    }
                }
            }
        }
    }

    println!("[SIMPLE] === SIMPLE TOOL EXTRACTION COMPLETE ===");
    println!("[SIMPLE] Found {} simple tools", tools.len());
    tools
}

fn extract_between_quotes(text: &str) -> Option<String> {
    let text = text.trim();

    if text.starts_with('"') {
        if let Some(end) = text[1..].find('"') {
            return Some(text[1..1 + end].to_string());
        }
    } else if text.starts_with('\'') {
        if let Some(end) = text[1..].find('\'') {
            return Some(text[1..1 + end].to_string());
        }
    }

    None
}

fn execute_tool(tool: &str, param: &str, root: &str) -> String {
    match tool {
        "read_file" => {
            let path = Path::new(root).join(param);
            println!("[EXECUTE] Reading file: {}", path.display());
            fs::read_to_string(&path).unwrap_or_else(|e| format!("Error: {}", e))
        }
        "write_file_delta" => {
            println!("[EXECUTE] Writing file delta: {}", param);
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
            println!("[EXECUTE] Running command: {}", param);
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
        _ => {
            println!("[ERROR] Unknown tool: {}", tool);
            String::new()
        }
    }
}

fn apply_delta(path: &Path, old_content: &str, new_content: &str) -> String {
    let existing_content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).ok();
            }
            return match fs::write(path, new_content) {
                Ok(_) => format!("Created new file {}", path.display()),
                Err(e) => format!("Error creating file: {}", e),
            };
        }
    };

    if old_content.is_empty() {
        return match fs::write(path, new_content) {
            Ok(_) => format!("Replaced entire file {}", path.display()),
            Err(e) => format!("Error writing file: {}", e),
        };
    }

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
            "Error: Could not find the specified content in {}",
            path.display()
        )
    }
}
