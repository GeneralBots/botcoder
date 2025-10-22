pub struct ResponseParser;

impl ResponseParser {
    pub fn new() -> Self {
        Self
    }
    
    pub fn extract_tools(&self, text: &str) -> Vec<(String, String)> {
        let cleaned = text
            .replace("```rust", "")
            .replace("```sh", "")
            .replace("```bash", "")
            .replace("```", "");
        
        let mut tools = Vec::new();
        
        let delta_tools = self.extract_delta_format(&cleaned);
        if !delta_tools.is_empty() {
            return delta_tools;
        }
        
        tools.extend(self.extract_simple_tools(&cleaned));
        tools
    }
    
    fn extract_delta_format(&self, text: &str) -> Vec<(String, String)> {
        let mut tools = Vec::new();
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        
        while i < lines.len() {
            let line = lines[i].trim();
            
            if line.starts_with("CHANGE:") {
                let file_path = line.replace("CHANGE:", "").trim().to_string();
                let mut current_content = String::new();
                let mut new_content = String::new();
                
                i += 1;
                
                while i < lines.len() && !lines[i].trim().starts_with("<<<<<<< CURRENT") {
                    i += 1;
                }
                
                if i >= lines.len() {
                    break;
                }
                
                i += 1;
                
                while i < lines.len() && !lines[i].trim().starts_with("=======") {
                    current_content.push_str(lines[i]);
                    current_content.push('\n');
                    i += 1;
                }
                
                if i >= lines.len() {
                    break;
                }
                
                i += 1;
                
                while i < lines.len() && !lines[i].trim().starts_with(">>>>>>> NEW") {
                    new_content.push_str(lines[i]);
                    new_content.push('\n');
                    i += 1;
                }
                
                if i >= lines.len() {
                    break;
                }
                
                i += 1;
                
                let tool_param = format!("{}:::{}\n{}", 
                    file_path, 
                    current_content.trim(), 
                    new_content.trim()
                );
                
                tools.push(("write_file_delta".to_string(), tool_param));
            } else {
                i += 1;
            }
        }
        
        tools
    }
    
    fn extract_simple_tools(&self, text: &str) -> Vec<(String, String)> {
        let mut tools = Vec::new();
        
        for line in text.lines() {
            let line = line.trim();
            
            if line.is_empty() 
                || line.starts_with("CHANGE:")
                || line.starts_with("<<<<<<<")
                || line.starts_with("=======")
                || line.starts_with(">>>>>>>") {
                continue;
            }
            
            if line.contains("read_file") {
                if let Some(param) = self.extract_tool_param(line, "read_file") {
                    tools.push(("read_file".to_string(), param));
                    continue;
                }
            }
            
            if line.contains("execute_command") {
                if let Some(param) = self.extract_tool_param(line, "execute_command") {
                    tools.push(("execute_command".to_string(), param));
                    continue;
                }
            }
        }
        
        tools
    }
    
    fn extract_tool_param(&self, line: &str, tool: &str) -> Option<String> {
        if let Some(start) = line.find(&format!("{}(", tool)) {
            if let Some(end) = line[start..].find(')') {
                let param = line[start + tool.len() + 1..start + end]
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                if !param.is_empty() {
                    return Some(param);
                }
            }
        }
        
        if let Some(start) = line.find(&format!("{}:", tool)) {
            let after = line[start + tool.len() + 1..].trim();
            return self.extract_between_quotes(after);
        }
        
        None
    }
    
    fn extract_between_quotes(&self, text: &str) -> Option<String> {
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
}
