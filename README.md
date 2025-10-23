# 🤖 BotCoder - AI Coding Agent

A beautiful, terminal-based AI coding agent with a modern dark theme UI.

## Features

- 🎨 Modern, fluid terminal UI with smooth animations
- 💬 Interactive chat interface
- 🛠️ Automated tool execution (file reading, writing, command execution)
- 📊 Real-time token usage statistics
- ⚡ Built-in TPM (Tokens Per Minute) rate limiting
- 🎯 Success detection for task completion

## Setup

1. Copy `.env.example` to `.env`:
   ```bash
   cp .env.example .env
   ```

2. Configure your environment variables in `.env`:
   - `LLM_URL`: Your Azure OpenAI endpoint
   - `LLM_KEY`: Your API key
   - `LLM_MODEL`: Model deployment name
   - `PROJECT_PATH`: Path to the project you want to work on

3. Build and run:
   ```bash
   cargo build --release
   cargo run
   ```

## Usage

### Keyboard Controls

- **Enter**: Send message to AI
- **Q / ESC / Ctrl+C**: Quit application
- **↑ / ↓**: Scroll through AI thoughts
- **PgUp / PgDn**: Fast scroll
- **Type**: Enter your message

### Tool Commands

The AI can use the following tools:

1. **Read files**:
   ```
   read_file("path/to/file")
   ```

2. **Execute commands**:
   ```
   execute_command("cargo build")
   ```

3. **Modify files**:
   ```
   CHANGE: path/to/file
   <<<<<<< CURRENT
   old content
   =======
   new content
   >>>>>>> NEW
   ```

## Configuration

Edit `prompt.txt` to customize the AI's behavior and instructions.

## Architecture

- **main.rs**: Application entry point and event loop
- **app.rs**: Application state and tool execution logic
- **llm.rs**: Azure OpenAI client with rate limiting
- **tpm_limiter.rs**: Token-per-minute rate limiter
- **ui.rs**: Terminal UI rendering with Ratatui

## Requirements

- Rust 1.70+
- Azure OpenAI API access (or compatible endpoint)

## License

MIT
