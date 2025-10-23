use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        BarChart, Block, Borders, Gauge, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
    Frame,
};

use crate::app::AppState;

pub fn draw_ui(f: &mut Frame, app: &AppState, spinner: &str) {
    // Modern dark theme color palette
    let bg = Color::Rgb(25, 28, 35);
    let border = Color::Rgb(70, 80, 95);
    let title_bar = Color::Rgb(45, 55, 72);
    let title_text = Color::Rgb(230, 237, 243);
    let text = Color::Rgb(203, 213, 225);
    let highlight = Color::Rgb(56, 189, 248);
    let success = Color::Rgb(34, 197, 94);
    let warning = Color::Rgb(251, 191, 36);
    let error = Color::Rgb(239, 68, 68);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(f.area());

    // Header
    let status_color = if app.processing { warning } else { success };
    let status_text = if app.processing {
        "⚡ Processing"
    } else {
        "✓ Ready"
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled("🤖 ", Style::default().fg(highlight)),
        Span::styled(
            "BOTCODER ",
            Style::default()
                .fg(title_text)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("| ", Style::default().fg(border)),
        Span::styled(
            format!("Iteration #{} ", app.iteration),
            Style::default().fg(text),
        ),
        Span::styled("| ", Style::default().fg(border)),
        Span::styled(status_text, Style::default().fg(status_color)),
        Span::styled(
            format!(" {} ", spinner),
            Style::default().fg(highlight),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border))
            .style(Style::default().bg(title_bar)),
    )
    .alignment(ratatui::layout::Alignment::Center);

    f.render_widget(header, chunks[0]);

    // Main content panels
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(40),
            Constraint::Percentage(20),
        ])
        .split(chunks[1]);

    // AI Thoughts panel
    let thoughts_block = Block::default()
        .title(" 💭 AI Thoughts ")
        .title_style(Style::default().fg(title_text).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(bg));

    let thoughts_lines: Vec<Line> = app
        .current_thoughts
        .lines()
        .map(|line| Line::from(Span::styled(line, Style::default().fg(text))))
        .collect();

    let thoughts_paragraph = Paragraph::new(thoughts_lines)
        .block(thoughts_block)
        .wrap(Wrap { trim: true })
        .scroll((app.thoughts_scroll as u16, 0));

    f.render_widget(thoughts_paragraph, main_chunks[0]);

    // Scrollbar for thoughts
    let thoughts_scrollbar = Scrollbar::default()
        .orientation(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("▲"))
        .end_symbol(Some("▼"))
        .track_symbol(Some("│"))
        .thumb_symbol("█");

    let mut thoughts_scrollbar_state = ScrollbarState::new(app.current_thoughts.lines().count())
        .position(app.thoughts_scroll as usize);

    f.render_stateful_widget(
        thoughts_scrollbar,
        main_chunks[0],
        &mut thoughts_scrollbar_state,
    );

    // Tools panel
    let tools_block = Block::default()
        .title(" 🛠️  Tool Execution ")
        .title_style(Style::default().fg(title_text).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(bg));

    let tool_items: Vec<ListItem> = app
        .current_tools
        .iter()
        .map(|(tool, param, result)| {
            let tool_color = match tool.as_str() {
                "read_file" => Color::Rgb(96, 165, 250),
                "write_file_delta" => Color::Rgb(251, 191, 36),
                "execute_command" => Color::Rgb(167, 139, 250),
                _ => text,
            };

            let tool_line = Line::from(vec![
                Span::styled(
                    format!("▸ {}: ", tool),
                    Style::default()
                        .fg(tool_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    if param.len() > 35 {
                        format!("{}...", &param[..32])
                    } else {
                        param.clone()
                    },
                    Style::default().fg(text),
                ),
            ]);

            let result_color = if result.contains("Error") || result.contains("✗") {
                error
            } else if result.contains("✓") {
                success
            } else {
                text
            };

            let result_preview = if result.len() > 55 {
                format!("  {}...", &result[..52])
            } else {
                format!("  {}", result)
            };

            let result_line = Line::from(Span::styled(
                result_preview,
                Style::default().fg(result_color),
            ));

            ListItem::new(vec![tool_line, result_line])
        })
        .collect();

    let tools_list = List::new(tool_items)
        .block(tools_block)
        .highlight_style(Style::default().bg(Color::Rgb(45, 55, 72)));

    f.render_widget(tools_list, main_chunks[1]);

    // Statistics panel
    let stats_block = Block::default()
        .title(" 📊 Statistics ")
        .title_style(Style::default().fg(title_text).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(bg));

    let stats_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(4),
            Constraint::Min(1),
        ])
        .split(main_chunks[2]);

    f.render_widget(stats_block, main_chunks[2]);

    // Token stats with icons
    let total_tokens = Paragraph::new(format!("🎯 Total: {}", app.stats.total_tokens))
        .style(Style::default().fg(text));
    f.render_widget(total_tokens, stats_chunks[0]);

    let tpm_color = if app.stats.current_tpm as f32 / app.stats.max_tpm as f32 > 0.8 {
        warning
    } else {
        success
    };
    let tpm_usage = Paragraph::new(format!(
        "⚡ TPM: {}/{}",
        app.stats.current_tpm, app.stats.max_tpm
    ))
    .style(Style::default().fg(tpm_color));
    f.render_widget(tpm_usage, stats_chunks[1]);

    let input_tokens = Paragraph::new(format!("📥 In: {}", app.stats.input_tokens))
        .style(Style::default().fg(Color::Rgb(96, 165, 250)));
    f.render_widget(input_tokens, stats_chunks[2]);

    let output_tokens = Paragraph::new(format!("📤 Out: {}", app.stats.output_tokens))
        .style(Style::default().fg(Color::Rgb(167, 139, 250)));
    f.render_widget(output_tokens, stats_chunks[3]);

    // TPM Gauge
    let tpm_percentage =
        (app.stats.current_tpm as f64 / app.stats.max_tpm as f64 * 100.0).min(100.0) as u16;
    let gauge = Gauge::default()
        .block(Block::default().style(Style::default().bg(bg)))
        .gauge_style(Style::default().fg(if tpm_percentage > 80 {
            error
        } else if tpm_percentage > 60 {
            warning
        } else {
            success
        }))
        .percent(tpm_percentage)
        .label(format!("{}%", tpm_percentage));
    f.render_widget(gauge, stats_chunks[4]);

    // Token distribution chart
    let data = vec![
        ("In", app.stats.input_tokens as u64),
        ("Out", app.stats.output_tokens as u64),
    ];
    let chart = BarChart::default()
        .block(Block::default().style(Style::default().bg(bg)))
        .data(&data)
        .bar_width(4)
        .bar_gap(2)
        .max(if app.stats.total_tokens > 0 {
            app.stats.total_tokens as u64
        } else {
            1
        })
        .style(Style::default().fg(text))
        .value_style(Style::default().fg(highlight));

    f.render_widget(chart, stats_chunks[5]);

    // Chat input
    let chat_block = Block::default()
        .title(" 💬 Message ")
        .title_style(Style::default().fg(title_text).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if app.processing {
            Color::Rgb(50, 50, 50)
        } else {
            border
        }))
        .style(Style::default().bg(bg));

    let input_display = if app.chat_input.is_empty() {
        "Type your message... (Enter to send)"
    } else {
        &app.chat_input
    };

    let chat_input = Paragraph::new(input_display)
        .block(chat_block)
        .style(if app.chat_input.is_empty() {
            Style::default().fg(Color::Rgb(100, 116, 139))
        } else {
            Style::default().fg(text)
        })
        .wrap(Wrap { trim: true });

    f.render_widget(chat_input, chunks[2]);

    // Footer
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" ▸ ", Style::default().fg(highlight)),
        Span::styled("Q/ESC", Style::default().fg(title_text).add_modifier(Modifier::BOLD)),
        Span::styled(": Quit ", Style::default().fg(text)),
        Span::styled("| ", Style::default().fg(border)),
        Span::styled("Enter", Style::default().fg(title_text).add_modifier(Modifier::BOLD)),
        Span::styled(": Send ", Style::default().fg(text)),
        Span::styled("| ", Style::default().fg(border)),
        Span::styled("↑↓", Style::default().fg(title_text).add_modifier(Modifier::BOLD)),
        Span::styled(": Scroll ", Style::default().fg(text)),
        Span::styled("| ", Style::default().fg(border)),
        Span::styled("PgUp/PgDn", Style::default().fg(title_text).add_modifier(Modifier::BOLD)),
        Span::styled(": Fast scroll", Style::default().fg(text)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().bg(title_bar).fg(text)),
    )
    .alignment(ratatui::layout::Alignment::Center);

    f.render_widget(footer, chunks[3]);

    // Success overlay
    if app.success_achieved {
        let area = centered_rect(50, 20, f.area());
        let success_block = Block::default()
            .title(" 🎉 SUCCESS! ")
            .title_style(
                Style::default()
                    .fg(success)
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(success))
            .style(Style::default().bg(Color::Rgb(20, 23, 30)));

        let success_text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "✓ Mission Accomplished!",
                Style::default()
                    .fg(success)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "All tasks completed successfully.",
                Style::default().fg(text),
            )),
            Line::from(""),
            Line::from(Span::styled("Press Q to exit", Style::default().fg(highlight))),
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
