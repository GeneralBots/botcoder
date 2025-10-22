use dotenv::dotenv;
use std::env;
use std::fs;
use std::io::{self};
use std::path::Path;
use std::process::Command;

mod llm;
use llm::AzureOpenAIClient;

use crate::llm::LLMProvider;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let instruction =
        env::var("INSTRUCTION").expect("Please set the INSTRUCTION variable in your .env file");

    // AzureOpenAIClient::new() now returns a Result; handle possible errors
    let client = match AzureOpenAIClient::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to create AzureOpenAIClient: {}", e);
            return;
        }
    };

    let prompt = fs::read_to_string("prompt.txt").expect("Failed to read prompt.txt");
    println!("Loaded prompt from prompt.txt");

    // Use PROJECT_PATH environment variable instead of prompting the user
    let project_root = env::var("PROJECT_PATH").expect("PROJECT_PATH not set");

    let mut iteration = 0;
    let mut conversation_history: Vec<String> = Vec::new();

    loop {
        iteration += 1;
        println!("\n=== ITERATION {} ===", iteration);

        // Build the context sent to the LLM
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

        println!("Sending request to LLM...");
        match client.generate(&context, &serde_json::json!({})).await {
            Ok(resp) => {
                // Ensure we have an owned `String` to avoid unsized `str` issues
                let response = resp.to_string();

                println!("\nAI Response:\n{}", response);
                conversation_history.push(format!("Assistant: {}", response));

                let tools = extract_tools(&response);
                println!("Extracted tools: {:?}", tools);

                if tools.is_empty() {
                    println!("\nNo tools found. Continue? (y/n): ");
                    let mut input = String::new();
                    io::stdin().read_line(&mut input).unwrap();
                    if input.trim() != "y" {
                        break;
                    }
                    continue;
                }

                let mut all_results = Vec::new();

                for (tool, param) in tools {
                    println!("Executing tool: {} with param: {}", tool, param);
                    let result = execute_tool(&tool, &param, &project_root);
                    println!("\nTool: {}\nResult:\n{}", tool, result);
                    all_results.push(format!("Tool: {}\nResult:\n{}", tool, result));

                    if tool == "execute_command" && param.contains("cargo run") {
                        if result.contains("exit_code: 0")
                            && !result.contains("error:")
                            && !result.contains("Error")
                        {
                            println!("\n=== SUCCESS ===");
                            return;
                        }
                    }
                }

                conversation_history.push(format!("Tool Results:\n{}", all_results.join("\n\n")));

                if conversation_history.len() > 20 {
                    conversation_history.drain(0..10);
                }
            }
            Err(err) => {
                // Convert error to a string for display
                let e = err.to_string();
                println!("Error calling LLM: {}", e);
                println!("Continue? (y/n): ");
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
    let mut tools = Vec::new();

    // ---------- read_file ----------
    if text.contains("read_file(") {
        for line in text.lines() {
            if let Some(start) = line.find("read_file(") {
                if let Some(end) = line[start..].find(')') {
                    let param = line[start + 10..start + end]
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'');
                    tools.push(("read_file".to_string(), param.to_string()));
                }
            }
        }
    }

    // ---------- write_file ----------
    if text.contains("write_file(") {
        let mut in_write = false;
        let mut path = String::new();
        let mut content = String::new();
        let mut delimiter: Option<String> = None; // ```` or `

        for line in text.lines() {
            if line.contains("write_file(") {
                in_write = true;
                // Extract path (first argument) up to the first comma
                if let Some(start) = line.find("write_file(") {
                    let rest = &line[start + 11..];
                    if let Some(comma) = rest.find(',') {
                        path = rest[..comma]
                            .trim()
                            .trim_matches('"')
                            .trim_matches('\'')
                            .to_string();
                        // Determine how the content starts (after the comma)
                        let after_comma = rest[comma + 1..].trim_start();
                        if after_comma.starts_with("```") {
                            delimiter = Some("```".to_string());
                            // If the opening delimiter is on the same line, start after it
                            content.push_str(after_comma.trim_start_matches("```"));
                            content.push('\n');
                            continue;
                        } else if after_comma.starts_with('`') {
                            delimiter = Some("`".to_string());
                            content.push_str(after_comma.trim_start_matches('`'));
                            content.push('\n');
                            continue;
                        }
                    }
                }
            } else if in_write {
                // Look for the closing delimiter
                if let Some(ref delim) = delimiter {
                    if line.trim_end().ends_with(delim) {
                        // Remove the closing delimiter
                        let line_without_delim = line.trim_end().trim_end_matches(delim);
                        content.push_str(line_without_delim);
                        // Store the tool
                        if !path.is_empty() && !content.is_empty() {
                            tools.push((
                                "write_file".to_string(),
                                format!("{}:::{}", path, content),
                            ));
                        }
                        // Reset state
                        in_write = false;
                        delimiter = None;
                        path.clear();
                        content.clear();
                        continue;
                    }
                }

                // Normal content line
                content.push_str(line);
                content.push('\n');
            }
        }
    }

    // ---------- execute_command ----------
    if text.contains("execute_command:") || text.contains("[execute_command:") {
        for line in text.lines() {
            if line.contains("execute_command:") {
                let cmd = if let Some(pos) = line.find("execute_command:") {
                    line[pos + 16..]
                        .trim()
                        .trim_matches('[')
                        .trim_matches(']')
                        .trim()
                } else {
                    continue;
                };
                tools.push(("execute_command".to_string(), cmd.to_string()));
            }
        }
    }

    tools
}

fn execute_tool(tool: &str, param: &str, root: &str) -> String {
    match tool {
        "read_file" => {
            let path = Path::new(root).join(param);
            fs::read_to_string(&path).unwrap_or_else(|e| format!("Error: {}", e))
        }
        "write_file" => {
            let parts: Vec<&str> = param.splitn(2, ":::").collect();
            if parts.len() == 2 {
                let path = Path::new(root).join(parts[0]);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).ok();
                }
                // Write the full content (including newlines) to the file
                fs::write(&path, parts[1]).unwrap_or_else(|e| {
                    panic!("Failed to write file {}: {}", path.display(), e);
                });
                format!("Written to {}", parts[0])
            } else {
                "Error: Invalid write_file format".to_string()
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
