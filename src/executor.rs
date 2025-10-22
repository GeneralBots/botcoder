use std::fs;
use std::path::Path;
use std::process::Command;

pub struct ToolExecutor {
    project_root: String,
}

impl ToolExecutor {
    pub fn new(project_root: String) -> Self {
        Self { project_root }
    }

    pub fn execute(&self, tool: &str, param: &str) -> String {
        match tool {
            "read_file" => self.read_file(param),
            "write_file_delta" => self.write_file_delta(param),
            "execute_command" => self.execute_command(param),
            _ => format!("Unknown tool: {}", tool),
        }
    }

    fn read_file(&self, path: &str) -> String {
        // Disallow absolute paths and parent directory components to prevent path traversal
        if Path::new(path).is_absolute() || path.contains("..") {
            return "Error: Unsafe file path".to_string();
        }

        let full_path = Path::new(&self.project_root).join(path);
        match fs::read_to_string(&full_path) {
            Ok(content) => content,
            Err(e) => format!("Error reading file: {}", e),
        }
    }

    fn write_file_delta(&self, param: &str) -> String {
        // Expected format: "<relative_path>:::<old_content>\n<new_content>"
        let parts: Vec<&str> = param.splitn(2, ":::").collect();
        if parts.len() != 2 {
            return "Error: Invalid delta format".to_string();
        }

        // Secure the target path
        let target_path_str = parts[0];
        if Path::new(target_path_str).is_absolute() || target_path_str.contains("..") {
            return "Error: Unsafe target file path".to_string();
        }
        let target_path = Path::new(&self.project_root).join(target_path_str);

        // Split the delta content into old and new parts
        let content_parts: Vec<&str> = parts[1].splitn(2, '\n').collect();
        if content_parts.len() != 2 {
            return "Error: Invalid delta content".to_string();
        }

        let old_content = content_parts[0].trim();
        let new_content = content_parts[1].trim();

        self.apply_delta(&target_path, old_content, new_content)
    }

    fn apply_delta(&self, path: &Path, old_content: &str, new_content: &str) -> String {
        let existing = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(_) => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).ok();
                }
                return match fs::write(path, new_content) {
                    Ok(_) => format!("Created new file: {}", path.display()),
                    Err(e) => format!("Error creating file: {}", e),
                };
            }
        };

        if old_content.is_empty() {
            return match fs::write(path, new_content) {
                Ok(_) => format!("Replaced entire file: {}", path.display()),
                Err(e) => format!("Error writing file: {}", e),
            };
        }

        if let Some(pos) = existing.find(old_content) {
            let mut updated = String::new();
            updated.push_str(&existing[..pos]);
            updated.push_str(new_content);
            updated.push_str(&existing[pos + old_content.len()..]);

            match fs::write(path, updated) {
                Ok(_) => format!("Applied delta to: {}", path.display()),
                Err(e) => format!("Error applying delta: {}", e),
            }
        } else {
            format!(
                "Error: Could not find specified content in {}",
                path.display()
            )
        }
    }

    fn execute_command(&self, cmd: &str) -> String {
        let output = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(&self.project_root)
            .output()
            .unwrap();

        format!(
            "stdout:\n{}\nstderr:\n{}\nexit_code: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            output.status.code().unwrap_or(-1)
        )
    }
}
