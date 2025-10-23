use std::{fs, path::Path, process::Command};

pub struct AppState {
    pub iteration: u32,
    pub conversation_history: Vec<String>,
    pub chat_input: String,
    pub current_thoughts: String,
    pub current_tools: Vec<(String, String, String)>,
    pub stats: Stats,
    pub should_quit: bool,
    pub success_achieved: bool,
    pub thoughts_scroll: u32,
    pub tools_scroll: u32,
    pub processing: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            iteration: 0,
            conversation_history: Vec::new(),
            current_thoughts: String::new(),
            current_tools: Vec::new(),
            stats: Stats::default(),
            should_quit: false,
            success_achieved: false,
            thoughts_scroll: 0,
            tools_scroll: 0,
            chat_input: String::new(),
            processing: false,
        }
    }
}

pub struct Stats {
    pub total_tokens: u32,
    pub current_tpm: u32,
    pub max_tpm: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            total_tokens: 0,
            current_tpm: 0,
            max_tpm: 20000,
            input_tokens: 0,
            output_tokens: 0,
        }
    }
}

pub fn count_tokens(text: &str) -> u32 {
    // Rough approximation: ~4 chars per token
    (text.len() / 4).max(text.split_whitespace().count()) as u32
}

pub fn filter_thinking_tokens(text: &str) -> String {
    text.replace("<|start|>assistant<|channel|>", "")
        .replace("<|message|>", "")
        .replace("<|end|>", "")
        .trim()
        .to_string()
}

pub fn extract_tools(text: &str) -> Vec<(String, String)> {
    let mut tools = Vec::new();

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
            if line.contains("execute_command(") {
                if let Some(start) = line.find("execute_command(") {
                    let after_open = &line[start + 16..];
                    if let Some(end) = after_open.find(')') {
                        let content = &after_open[..end];
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
            }
        }
    }

    // Extract file changes
    if text.contains("CHANGE:") {
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            if line.starts_with("CHANGE:") {
                let file_path = line.replace("CHANGE:", "").trim().to_string();
                let mut current_content = String::new();
                let mut new_content = String::new();
                let mut in_current = false;
                let mut in_new = false;

                i += 1;
                while i < lines.len() {
                    let current_line = lines[i];

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

                if !file_path.is_empty() {
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

    // Remove duplicates
    let mut unique_tools = Vec::new();
    for tool in tools {
        if !unique_tools.contains(&tool) {
            unique_tools.push(tool);
        }
    }

    unique_tools
}

pub fn execute_tool(tool: &str, param: &str, root: &str) -> String {
    match tool {
        "read_file" => {
            let path = Path::new(root).join(param);
            fs::read_to_string(&path).unwrap_or_else(|e| format!("Error reading file: {}", e))
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
                    "Error: Invalid delta format".to_string()
                }
            } else {
                "Error: Invalid write_file_delta format".to_string()
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
                Err(e) => format!("Error executing command: {}", e),
            }
        }
        _ => format!("Unknown tool: {}", tool),
    }
}

fn apply_delta(path: &Path, old_content: &str, new_content: &str) -> String {
    let existing_content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            return match fs::write(path, new_content) {
                Ok(_) => format!("✓ Created new file: {}", path.display()),
                Err(e) => format!("✗ Error creating file: {}", e),
            };
        }
    };

    if old_content.is_empty() {
        return match fs::write(path, new_content) {
            Ok(_) => format!("✓ Replaced entire file: {}", path.display()),
            Err(e) => format!("✗ Error replacing file: {}", e),
        };
    }

    if let Some(pos) = existing_content.find(old_content) {
        let mut updated_content = String::new();
        updated_content.push_str(&existing_content[..pos]);
        updated_content.push_str(new_content);
        updated_content.push_str(&existing_content[pos + old_content.len()..]);

        match fs::write(path, updated_content) {
            Ok(_) => format!("✓ Successfully applied delta to: {}", path.display()),
            Err(e) => format!("✗ Error applying delta: {}", e),
        }
    } else {
        format!(
            "✗ Could not find content in {}\nSearching for:\n{}",
            path.display(),
            old_content
        )
    }
}
