# 🤖 General Bots Coder

Terminal-based AI coding agent with real-time streaming and COMPLETE tool output (no truncation!)

## ✨ Key Features

- 🌊 **Real-time Streaming** - Watch AI responses appear live
- 📜 **Full Output Scrolling** - See EVERY line of tool output, scroll forever
- 🎨 **Animated Gradients** - Fluid UI animations
- 🛠️ **Tool Execution** - Reads files, executes commands, modifies code
- 📊 **Live Statistics** - Real-time token usage
- ⚡ **Non-blocking** - Smooth, responsive UI

## 🚀 Quick Start

```bash
bash restore.sh
cd botcoder
cp .env.example .env
# Edit .env with your Azure OpenAI credentials
cargo run
```

## ⌨️ Controls

- **Enter**: Send message (streams response!)
- **↑ / ↓ / PgUp / PgDn**: Scroll AI output panel
- **W / S**: Scroll tools panel
- **Q / ESC / Ctrl+C**: Quit

## 🎨 UI Layout

```
┌─────────────────────────────────────────────┐
│  Header: Status, Iteration, Streaming      │
├──────────────┬──────────────┬───────────────┤
│  AI Output   │  Tool Output │  Statistics   │
│  (50%)       │  (30%)       │  (20%)        │
│  FULL TEXT   │  NO TRUNCATE │  Tokens, TPM  │
│  Scrollable  │  Scrollable  │  Animated     │
├──────────────────────────────────────────────┤
│  Chat Input                                 │
├──────────────────────────────────────────────┤
│  Footer: Controls                           │
└──────────────────────────────────────────────┘
```

## 💡 Features

- **NO TRUNCATION**: All tool output shown in full
- **Infinite Scrolling**: Scroll through thousands of lines
- **Live Streaming**: Text appears character-by-character
- **Tool Detection**: Automatically extracts and executes tools
- **Color Coding**: ✓ success (green), ✗ errors (red), tools (yellow)
- **Line Counts**: Shows total lines in each panel

## 📝 License

MIT
