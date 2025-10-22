use crate::executor::ToolExecutor;
use crate::limiter::TPMLimiter;
use crate::llm::{AzureOpenAIClient, LLMProvider};
use crate::parser::ResponseParser;
use crate::tools::ToolRegistry;
use crossterm::{
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use std::env;
use std::io::{self, Write};

pub struct ChatSession {
    client: AzureOpenAIClient,
    registry: ToolRegistry,
    executor: ToolExecutor,
    parser: ResponseParser,
    limiter: TPMLimiter,
    history: Vec<Message>,
    project_path: String,
}

#[derive(Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl ChatSession {
    pub async fn new(project_path: String) -> Result<Self, String> {
        let client = AzureOpenAIClient::new().map_err(|e| e.to_string())?;
        let registry = ToolRegistry::new();
        let executor = ToolExecutor::new(project_path.clone());
        let parser = ResponseParser::new();

        let tpm_limit: u32 = env::var("LLM_TPM")
            .unwrap_or_else(|_| "20000".to_string())
            .parse()
            .unwrap_or(20000);

        let min_interval: u64 = env::var("LLM_MIN_INTERVAL")
            .unwrap_or_else(|_| "1".to_string())
            .parse()
            .unwrap_or(1);

        let limiter = TPMLimiter::new(tpm_limit, min_interval);

        Ok(Self {
            client,
            registry,
            executor,
            parser,
            limiter,
            history: Vec::new(),
            project_path,
        })
    }

    pub async fn run(&mut self) {
        self.print_welcome();

        loop {
            self.print_prompt();

            let input = match self.read_input() {
                Some(i) => i,
                None => continue,
            };

            if input.trim().is_empty() {
                continue;
            }

            if self.handle_command(&input) {
                continue;
            }

            self.history.push(Message {
                role: "user".to_string(),
                content: input.clone(),
            });

            if let Err(e) = self.process_turn().await {
                self.print_error(&format!("Error: {}", e));
            }
        }
    }

    fn print_welcome(&self) {
        let mut stdout = io::stdout();
        execute!(
            stdout,
            Clear(ClearType::All),
            SetForegroundColor(Color::Cyan),
            Print("╔════════════════════════════════════════╗\n"),
            Print("║   RUST CODE AGENT - Interactive Mode  ║\n"),
            Print("╚════════════════════════════════════════╝\n"),
            ResetColor,
            Print("\n"),
            SetForegroundColor(Color::Grey),
            Print("Project: "),
            SetForegroundColor(Color::White),
            Print(&self.project_path),
            Print("\n"),
            SetForegroundColor(Color::Grey),
            Print("Commands: /help /exit /clear /history\n\n"),
            ResetColor
        )
        .ok();
    }

    fn print_prompt(&self) {
        let mut stdout = io::stdout();
        execute!(
            stdout,
            SetForegroundColor(Color::Green),
            Print("You> "),
            ResetColor
        )
        .ok();
        stdout.flush().ok();
    }

    fn read_input(&self) -> Option<String> {
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok()?;
        Some(input.trim().to_string())
    }

    fn handle_command(&mut self, input: &str) -> bool {
        match input {
            "/exit" | "/quit" => {
                self.print_info("Goodbye!");
                std::process::exit(0);
            }
            "/clear" => {
                self.history.clear();
                self.print_info("History cleared");
                true
            }
            "/history" => {
                self.show_history();
                true
            }
            "/help" => {
                self.show_help();
                true
            }
            _ => false,
        }
    }

    async fn process_turn(&mut self) -> Result<(), String> {
        let context = self.build_context();
        let input_tokens = self.count_tokens(&context);

        self.limiter.wait_if_needed();

        self.print_thinking();

        let response = self
            .client
            .generate(&context, &serde_json::json!({}))
            .await
            .map_err(|e| e.to_string())?;

        let response_text = self.filter_response(&response.to_string());
        let output_tokens = self.count_tokens(&response_text);

        self.limiter.add_token_usage(input_tokens + output_tokens);

        let tools = self.parser.extract_tools(&response_text);

        if tools.is_empty() {
            self.print_assistant(&response_text);
            self.history.push(Message {
                role: "assistant".to_string(),
                content: response_text,
            });
            return Ok(());
        }

        self.print_assistant(&format!("Executing {} tool(s)...", tools.len()));

        let mut results = Vec::new();
        for (tool_name, param) in &tools {
            self.print_tool(tool_name, param);
            let result = self.executor.execute(tool_name, param);
            self.print_result(&result);
            results.push(format!("Tool: {}\nResult:\n{}", tool_name, result));
        }

        self.history.push(Message {
            role: "assistant".to_string(),
            content: response_text,
        });

        self.history.push(Message {
            role: "system".to_string(),
            content: format!("Tool Results:\n{}", results.join("\n\n")),
        });

        if self.history.len() > 40 {
            self.history.drain(0..20);
        }

        Ok(())
    }

    fn build_context(&self) -> String {
        let system_prompt = self.registry.get_system_prompt();

        let mut context = format!("{}\n\nProject: {}\n\n", system_prompt, self.project_path);

        for msg in &self.history {
            context.push_str(&format!("{}: {}\n\n", msg.role, msg.content));
        }

        context.push_str("Assistant:");
        context
    }

    fn count_tokens(&self, text: &str) -> u32 {
        (text.len() / 4) as u32
    }

    fn filter_response(&self, text: &str) -> String {
        text.replace("<|start|>assistant<|channel|>", "")
            .replace("<|message|>", "")
            .replace("<|end|>", "")
            .trim()
            .to_string()
    }

    fn print_assistant(&self, text: &str) {
        let mut stdout = io::stdout();
        execute!(
            stdout,
            SetForegroundColor(Color::Blue),
            Print("Agent> "),
            ResetColor,
            Print(text),
            Print("\n\n")
        )
        .ok();
    }

    fn print_thinking(&self) {
        let mut stdout = io::stdout();
        execute!(
            stdout,
            SetForegroundColor(Color::Yellow),
            Print("Agent> "),
            SetForegroundColor(Color::Grey),
            Print("[thinking...]\n"),
            ResetColor
        )
        .ok();
    }

    fn print_tool(&self, tool: &str, param: &str) {
        let mut stdout = io::stdout();
        let preview = if param.len() > 60 {
            format!("{}...", &param[..57])
        } else {
            param.to_string()
        };
        execute!(
            stdout,
            SetForegroundColor(Color::Magenta),
            Print("  [TOOL] "),
            SetForegroundColor(Color::White),
            Print(tool),
            SetForegroundColor(Color::Grey),
            Print(": "),
            Print(&preview),
            Print("\n"),
            ResetColor
        )
        .ok();
    }

    fn print_result(&self, result: &str) {
        let mut stdout = io::stdout();
        let preview = if result.len() > 200 {
            format!("{}...", &result[..197])
        } else {
            result.to_string()
        };
        execute!(
            stdout,
            SetForegroundColor(Color::Green),
            Print("  [RESULT] "),
            SetForegroundColor(Color::Grey),
            Print(&preview),
            Print("\n\n"),
            ResetColor
        )
        .ok();
    }

    fn print_info(&self, text: &str) {
        let mut stdout = io::stdout();
        execute!(
            stdout,
            SetForegroundColor(Color::Cyan),
            Print("Info: "),
            ResetColor,
            Print(text),
            Print("\n\n")
        )
        .ok();
    }

    fn print_error(&self, text: &str) {
        let mut stdout = io::stdout();
        execute!(
            stdout,
            SetForegroundColor(Color::Red),
            Print("Error: "),
            ResetColor,
            Print(text),
            Print("\n\n")
        )
        .ok();
    }

    fn show_history(&self) {
        let mut stdout = io::stdout();
        execute!(
            stdout,
            SetForegroundColor(Color::Cyan),
            Print("\n=== Conversation History ===\n\n"),
            ResetColor
        )
        .ok();

        for msg in &self.history {
            let color = match msg.role.as_str() {
                "user" => Color::Green,
                "assistant" => Color::Blue,
                _ => Color::Grey,
            };
            execute!(
                stdout,
                SetForegroundColor(color),
                Print(&format!("{}: ", msg.role)),
                ResetColor,
                Print(&msg.content),
                Print("\n\n")
            )
            .ok();
        }
    }

    fn show_help(&self) {
        let mut stdout = io::stdout();
        execute!(
            stdout,
            SetForegroundColor(Color::Cyan),
            Print("\n=== Available Commands ===\n\n"),
            ResetColor,
            Print("/help     - Show this help\n"),
            Print("/exit     - Exit the agent\n"),
            Print("/clear    - Clear conversation history\n"),
            Print("/history  - Show conversation history\n\n"),
            SetForegroundColor(Color::Cyan),
            Print("=== Available Tools ===\n\n"),
            ResetColor,
            Print("read_file: \"path\"         - Read file contents\n"),
            Print("execute_command: \"cmd\"    - Run shell command\n"),
            Print("CHANGE: path              - Modify file (delta format)\n\n")
        )
        .ok();
    }
}
