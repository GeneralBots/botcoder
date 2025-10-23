use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        BarChart, Block, Borders, Dataset, Gauge, List, ListItem, Paragraph, Scrollbar,
        ScrollbarState, Wrap,
    },
    Frame,
};

use crate::app::AppState;

pub fn draw_ui(f: &mut Frame, app: &AppState, spinner: &str) {
    // Windows 3.1 color palette
    let window_bg = Color::Rgb(192, 192, 192); // Light gray background
    let window_frame = Color::Rgb(0, 0, 0); // Black borders
    let title_bar = Color::Rgb(0, 0, 128); // Dark blue title bars
    let title_text = Color::Rgb(255, 255, 255); // White title text
    let button_face = Color::Rgb(192, 192, 192); // Button color
    let button_shadow = Color::Rgb(128, 128, 128); // Dark gray for shadows
    let button_highlight = Color::Rgb(255, 255, 255); // White for highlights
    let text_color = Color::Rgb(0, 0, 0); // Black text
    let highlight_bg = Color::Rgb(0, 0, 128); // Dark blue for highlights
    let highlight_text = Color::Rgb(255, 255, 255); // White text on highlights

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),   // Main content
            Constraint::Length(3), // Chat input
            Constraint::Length(3), // Footer
        ])
        .split(f.area());

    // Header - Windows 3.1 style title bar
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " 🤖 GENERAL BOTS ",
            Style::default()
                .fg(title_text)
                .bg(title_bar)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " AI CODING AGENT ",
            Style::default().fg(title_text).bg(title_bar),
        ),
        Span::styled(
            format!(" Iteration: {} ", app.iteration),
            Style::default().fg(title_text).bg(Color::Rgb(0, 128, 128)), // Teal
        ),
        Span::styled(
            format!(" {} ", spinner),
            Style::default().fg(Color::Rgb(255, 255, 0)), // Yellow spinner
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(window_frame))
            .style(Style::default().bg(title_bar)),
    )
    .alignment(ratatui::layout::Alignment::Center);

    f.render_widget(header, chunks[0]);

    // Main content - split into 3 panels
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40), // Thoughts panel
            Constraint::Percentage(40), // Tools panel
            Constraint::Percentage(20), // Stats panel
        ])
        .split(chunks[1]);

    // Panel 1: AI Thoughts with scroll - Windows 3.1 style
    let thoughts_block = Block::default()
        .title(" 💭 AI THOUGHTS ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(window_frame))
        .style(Style::default().bg(window_bg).fg(text_color));

    let thoughts_text: Vec<Line> = app
        .current_thoughts
        .lines()
        .map(|line| Line::from(Span::styled(line, Style::default().fg(text_color))))
        .collect();

    let thoughts_paragraph = Paragraph::new(thoughts_text)
        .block(thoughts_block)
        .wrap(Wrap { trim: true })
        .scroll((app.thoughts_scroll as u16, 0));

    f.render_widget(thoughts_paragraph, main_chunks[0]);

    // Add scrollbar for thoughts
    let thoughts_scrollbar = Scrollbar::default()
        .orientation(ratatui::widgets::ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let mut thoughts_scrollbar_state = ScrollbarState::new(app.current_thoughts.lines().count())
        .position(app.thoughts_scroll as usize);

    f.render_stateful_widget(
        thoughts_scrollbar,
        main_chunks[0],
        &mut thoughts_scrollbar_state,
    );

    // Panel 2: Tool Execution with scroll - Windows 3.1 style
    let tools_block = Block::default()
        .title(" 🛠️ TOOL EXECUTION ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(window_frame))
        .style(Style::default().bg(window_bg).fg(text_color));

    let tool_items: Vec<ListItem> = app
        .current_tools
        .iter()
        .map(|(tool, param, result)| {
            let tool_line = Line::from(vec![
                Span::styled(
                    format!("{}: ", tool),
                    Style::default()
                        .fg(Color::Rgb(0, 0, 128))
                        .add_modifier(Modifier::BOLD), // Dark blue
                ),
                Span::styled(
                    if param.len() > 30 {
                        format!("{}...", &param[..27])
                    } else {
                        param.clone()
                    },
                    Style::default().fg(text_color),
                ),
            ]);

            let result_preview = if result.len() > 50 {
                format!("{}...", &result[..47])
            } else {
                result.clone()
            };

            let result_line = Line::from(vec![
                Span::styled("Result: ", Style::default().fg(Color::Rgb(0, 128, 0))), // Green
                Span::styled(result_preview, Style::default().fg(text_color)),
            ]);

            ListItem::new(vec![tool_line, result_line])
        })
        .collect();

    let tools_list = List::new(tool_items)
        .block(tools_block)
        .highlight_style(Style::default().bg(highlight_bg).fg(highlight_text));

    f.render_widget(tools_list, main_chunks[1]);

    // Add scrollbar for tools
    let tools_scrollbar = Scrollbar::default()
        .orientation(ratatui::widgets::ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let mut tools_scrollbar_state =
        ScrollbarState::new(app.current_tools.len() * 2).position(app.tools_scroll as usize);

    f.render_stateful_widget(tools_scrollbar, main_chunks[1], &mut tools_scrollbar_state);

    // Panel 3: Statistics - Windows 3.1 style
    let stats_block = Block::default()
        .title(" 📊 STATISTICS ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(window_frame))
        .style(Style::default().bg(window_bg).fg(text_color));

    let stats_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(6),
            Constraint::Min(1),
        ])
        .split(main_chunks[2]);

    // Token stats
    let total_tokens = Paragraph::new(format!("Total Tokens: {}", app.stats.total_tokens))
        .style(Style::default().fg(Color::Rgb(0, 128, 0))); // Green
    f.render_widget(total_tokens, stats_chunks[0]);

    let tpm_usage = Paragraph::new(format!(
        "TPM: {}/{}",
        app.stats.current_tpm, app.stats.max_tpm
    ))
    .style(Style::default().fg(Color::Rgb(128, 0, 0))); // Maroon
    f.render_widget(tpm_usage, stats_chunks[1]);

    let input_tokens = Paragraph::new(format!("Input: {} tokens", app.stats.input_tokens))
        .style(Style::default().fg(Color::Rgb(0, 0, 128))); // Dark blue
    f.render_widget(input_tokens, stats_chunks[2]);

    let output_tokens = Paragraph::new(format!("Output: {} tokens", app.stats.output_tokens))
        .style(Style::default().fg(Color::Rgb(128, 0, 128))); // Purple
    f.render_widget(output_tokens, stats_chunks[3]);

    // TPM Gauge - Windows 3.1 style
    let tpm_percentage =
        (app.stats.current_tpm as f64 / app.stats.max_tpm as f64 * 100.0).min(100.0) as u16;
    let gauge = Gauge::default()
        .block(
            Block::default()
                .title("TPM Usage")
                .style(Style::default().bg(window_bg)),
        )
        .gauge_style(
            Style::default()
                .fg(if tpm_percentage > 80 {
                    Color::Rgb(255, 0, 0) // Red
                } else {
                    Color::Rgb(0, 128, 0) // Green
                })
                .bg(button_shadow),
        )
        .percent(tpm_percentage);
    f.render_widget(gauge, stats_chunks[4]);

    // Token distribution chart
    let data = vec![
        ("Input", app.stats.input_tokens as u64),
        ("Output", app.stats.output_tokens as u64),
    ];
    let chart = BarChart::default()
        .block(
            Block::default()
                .title("Token Distribution")
                .style(Style::default().bg(window_bg)),
        )
        .data(data.as_slice())
        .bar_width(6)
        .bar_gap(1)
        .group_gap(3)
        .max(if app.stats.total_tokens > 0 {
            app.stats.total_tokens as u64
        } else {
            1
        })
        .style(Style::default().fg(text_color))
        .value_style(Style::default().fg(highlight_text).bg(highlight_bg));

    f.render_widget(chart, stats_chunks[5]);

    // Chat input area - Windows 3.1 style
    let chat_block = Block::default()
        .title(" 💬 CHAT INPUT ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(window_frame))
        .style(Style::default().bg(window_bg).fg(text_color));

    let input_display = if app.chat_input.is_empty() {
        "Type your message here... (Press Enter to send)"
    } else {
        &app.chat_input
    };

    let chat_input = Paragraph::new(input_display)
        .block(chat_block)
        .style(if app.chat_input.is_empty() {
            Style::default().fg(button_shadow) // Gray placeholder text
        } else {
            Style::default().fg(text_color) // Black text when typing
        })
        .wrap(Wrap { trim: true });

    f.render_widget(chat_input, chunks[2]);

    // Footer - Windows 3.1 style
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" Q: Quit ", Style::default().fg(Color::Rgb(255, 0, 0))), // Red
        Span::raw(" | "),
        Span::styled(
            "Enter: Send message",
            Style::default().fg(Color::Rgb(0, 128, 0)),
        ), // Green
        Span::raw(" | "),
        Span::styled("ESC: Exit", Style::default().fg(Color::Rgb(128, 0, 0))), // Maroon
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().bg(button_face).fg(text_color)),
    )
    .alignment(ratatui::layout::Alignment::Center);

    f.render_widget(footer, chunks[3]);

    // Success overlay - Windows 3.1 style
    if app.success_achieved {
        let area = centered_rect(60, 25, f.area());
        let success_block = Block::default()
            .title(" 🎉 MISSION ACCOMPLISHED! ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(window_frame))
            .style(Style::default().bg(window_bg).fg(text_color));

        let success_text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "✓ TASK COMPLETED SUCCESSFULLY!",
                Style::default()
                    .fg(Color::Rgb(0, 128, 0))
                    .add_modifier(Modifier::BOLD), // Green
            )),
            Line::from(""),
            Line::from(Span::styled(
                "All objectives have been achieved!",
                Style::default().fg(text_color),
            )),
            Line::from(Span::styled(
                "The agent has successfully completed its mission.",
                Style::default().fg(text_color),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press 'Q' to exit",
                Style::default().fg(Color::Rgb(128, 0, 0)), // Maroon
            )),
        ];

        let success_paragraph = Paragraph::new(success_text)
            .block(success_block)
            .alignment(ratatui::layout::Alignment::Center);

        f.render_widget(success_paragraph, area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
