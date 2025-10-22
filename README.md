# Rust Code Agent - Interactive Chat System

Professional interactive coding agent similar to Claude Code, powered by Azure OpenAI.

## Features

- Interactive chat interface with colored output
- Conversation history management
- Rate limiting with TPM control
- File operations (read, write with delta format)
- Command execution in project context
- Multi-file architecture with clean separation

## Setup

1. Copy `.env.example` to `.env`
2. Configure Azure OpenAI credentials
3. Set PROJECT_PATH to your target project
4. Run: `cargo run`

## Usage

Start the agent and interact naturally:
```
You> create a new rust library with add function
Agent> [executing tools...]
You> test it with cargo test
Agent> [executing tools...]
```

## Commands

- `/help` - Show available commands
- `/exit` - Exit the agent
- `/clear` - Clear conversation history
- `/history` - Show conversation history

## Tools

Agent automatically uses:
- `read_file: "path"` - Read file contents
- `execute_command: "cmd"` - Run shell commands
- `CHANGE: path` with delta format - Modify files

## Architecture

- `main.rs` - Entry point
- `chat.rs` - Interactive session management
- `llm.rs` - Azure OpenAI client
- `tools.rs` - Tool registry and system prompts
- `parser.rs` - Response parsing and tool extraction
- `executor.rs` - Tool execution logic
- `limiter.rs` - Rate limiting (TPM control)
