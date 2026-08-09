use crate::state::{App, BlameCacheState, BranchState, ChangedFilesState, DiffState, Focus};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn draw(f: &mut Frame, app: &mut App) {
    let size = f.area();

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(size);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(main_chunks[0]);

    let search = Paragraph::new(format!(" {}_", app.query)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Search — type to filter, @name for author, Tab focus, ESC quit ")
            .border_style(Style::default().fg(Color::Yellow)),
    );
    f.render_widget(search, left_chunks[0]);

    let status_text = if app.status.is_empty() {
        " Enter — copy hash".to_string()
    } else {
        format!(" ✓ {}", app.status)
    };
    let status = Paragraph::new(status_text).style(Style::default().fg(if app.status.is_empty() {
        Color::DarkGray
    } else {
        Color::Green
    }));
    f.render_widget(status, left_chunks[1]);

    let items: Vec<ListItem> = app
        .filtered
        .iter()
        .map(|&i| {
            let c = &app.commits[i];
            let color = author_color(&c.author);
            let line = Line::from(vec![
                Span::styled(c.graph.clone(), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{} ", c.short_hash),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(truncate(&c.subject, 18), Style::default().fg(Color::White)),
                Span::styled(
                    format!(" · {}", truncate(&c.author, 10)),
                    Style::default().fg(color),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list_border_color = if app.focus == Focus::List {
        Color::Yellow
    } else {
        Color::Cyan
    };
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} commits ", app.filtered.len()))
                .border_style(Style::default().fg(list_border_color)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, left_chunks[2], &mut app.list_state);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(0)])
        .split(main_chunks[1]);

    draw_changed_files(f, app, right_chunks[0]);
    draw_preview(f, app, right_chunks[1]);
}

fn draw_changed_files(f: &mut Frame, app: &mut App, area: Rect) {
    let border_color = if app.focus == Focus::Files {
        Color::Yellow
    } else {
        Color::Cyan
    };

    let Some(hash) = app.selected_hash() else {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Changed files ")
            .border_style(Style::default().fg(border_color));
        f.render_widget(Paragraph::new("No commit selected").block(block), area);
        return;
    };

    match app.cache.changed_files_for(&hash) {
        None | Some(ChangedFilesState::Loading) => {
            let block = Block::default()
                .borders(Borders::ALL)
                .title(" Changed files ")
                .border_style(Style::default().fg(border_color));
            f.render_widget(
                Paragraph::new("Loading…")
                    .style(Style::default().fg(Color::DarkGray))
                    .block(block),
                area,
            );
        }
        Some(ChangedFilesState::Failed(e)) => {
            let block = Block::default()
                .borders(Borders::ALL)
                .title(" Changed files ")
                .border_style(Style::default().fg(border_color));
            f.render_widget(
                Paragraph::new(format!("Failed to load files: {}", e))
                    .style(Style::default().fg(Color::Red))
                    .block(block),
                area,
            );
        }
        Some(ChangedFilesState::Loaded(files)) => {
            let items: Vec<ListItem> = files
                .iter()
                .map(|cf| {
                    let (status_color, label) = status_style(&cf.status);
                    let stats = match (cf.additions, cf.deletions) {
                        (Some(a), Some(d)) => format!("+{} -{}", a, d),
                        _ => "bin".to_string(),
                    };
                    let line = Line::from(vec![
                        Span::styled(format!("{:<2}", label), Style::default().fg(status_color)),
                        Span::styled(truncate(&cf.path, 30), Style::default().fg(Color::White)),
                        Span::styled(format!(" {}", stats), Style::default().fg(Color::DarkGray)),
                    ]);
                    ListItem::new(line)
                })
                .collect();

            let title = format!(" {} changed files (b: blame) ", files.len());
            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .border_style(Style::default().fg(border_color)),
                )
                .highlight_style(
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");

            f.render_stateful_widget(list, area, &mut app.files_list_state);
        }
    }
}

fn status_style(status: &str) -> (Color, String) {
    let c = status.chars().next().unwrap_or('?');
    let color = match c {
        'A' => Color::Green,
        'D' => Color::Red,
        'M' => Color::Yellow,
        'R' => Color::Magenta,
        'C' => Color::Cyan,
        _ => Color::Gray,
    };
    (color, c.to_string())
}

fn draw_preview(f: &mut Frame, app: &mut App, area: Rect) {
    if app.blame_view.is_some() {
        draw_blame(f, app, area);
    } else {
        draw_diff(f, app, area);
    }
}

fn draw_diff(f: &mut Frame, app: &App, area: Rect) {
    let diff_lines = build_diff_lines(app);

    let diff_border_color = if app.focus == Focus::Preview {
        Color::Yellow
    } else {
        Color::Cyan
    };
    let diff = Paragraph::new(diff_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Diff (Tab to focus, ↑↓ / PgUp/PgDn to scroll) ")
                .border_style(Style::default().fg(diff_border_color)),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.diff_scroll, 0));

    f.render_widget(diff, area);
}

fn draw_blame(f: &mut Frame, app: &mut App, area: Rect) {
    let inner_height = area.height.saturating_sub(2) as usize;

    let (commit_hash, file_path, selected_line) = {
        let bv = app.blame_view.as_ref().unwrap();
        (
            bv.commit_hash.clone(),
            bv.file_path.clone(),
            bv.selected_line,
        )
    };

    let total_lines = match app.cache.blame_for(&commit_hash, &file_path) {
        Some(BlameCacheState::Loaded(lines)) => lines.len(),
        _ => 0,
    };

    if let Some(bv) = app.blame_view.as_mut()
        && inner_height > 0
    {
        if selected_line < bv.scroll_offset as usize {
            bv.scroll_offset = selected_line as u16;
        } else if selected_line >= bv.scroll_offset as usize + inner_height {
            bv.scroll_offset = (selected_line + 1 - inner_height) as u16;
        }
        let max_offset = total_lines.saturating_sub(inner_height) as u16;
        if bv.scroll_offset > max_offset {
            bv.scroll_offset = max_offset;
        }
    }

    let scroll_offset = app.blame_view.as_ref().unwrap().scroll_offset as usize;

    let title = format!(
        " Blame: {} @ {} (Esc: back) ",
        truncate(&file_path, 30),
        &commit_hash[..commit_hash.len().min(7)]
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Yellow));

    let content_width = (area.width as usize).saturating_sub(38);

    let lines: Vec<Line> = match app.cache.blame_for(&commit_hash, &file_path) {
        None | Some(BlameCacheState::Loading) => {
            vec![Line::from(Span::styled(
                "Loading blame…",
                Style::default().fg(Color::DarkGray),
            ))]
        }
        Some(BlameCacheState::Failed(e)) => {
            vec![Line::from(Span::styled(
                format!(
                    "Could not load blame (file may not exist at this commit, or was renamed/deleted): {}",
                    e
                ),
                Style::default().fg(Color::Red),
            ))]
        }
        Some(BlameCacheState::Loaded(blame_lines)) => blame_lines
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(inner_height.max(1))
            .map(|(i, bl)| {
                let is_selected = i == selected_line;
                let base_style = if is_selected {
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                Line::from(vec![
                    Span::styled(
                        format!("{:>5} ", bl.line_no),
                        base_style.fg(Color::DarkGray),
                    ),
                    Span::styled(format!("{} ", bl.short_hash), base_style.fg(Color::Yellow)),
                    Span::styled(
                        format!("{:<12} ", truncate(&bl.author, 12)),
                        base_style.fg(author_color(&bl.author)),
                    ),
                    Span::styled(
                        format!("{} ", format_unix_date(bl.author_time)),
                        base_style.fg(Color::DarkGray),
                    ),
                    Span::styled(
                        truncate(&bl.content, content_width.max(10)),
                        base_style.fg(Color::White),
                    ),
                ])
            })
            .collect(),
    };

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn format_unix_date(ts: i64) -> String {
    let days = ts.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Howard Hinnant's `civil_from_days`: converts a day count since the Unix
/// epoch into a proleptic-Gregorian (year, month, day), pure integer math.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn build_diff_lines(app: &App) -> Vec<Line<'_>> {
    let Some(commit) = app.selected_commit() else {
        return vec![Line::from("No commit selected")];
    };

    let branch_label = match app.cache.branch(&commit.hash) {
        Some(BranchState::Loaded(b)) => b.clone(),
        _ => String::new(),
    };

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(
                format!("commit {}   by ", commit.short_hash),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                commit.author.clone(),
                Style::default()
                    .fg(author_color(&commit.author))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" <{}>", commit.author_email),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{}  ", commit.date),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                if branch_label.is_empty() {
                    String::new()
                } else {
                    format!("⎇ {}", branch_label)
                },
                Style::default().fg(Color::Magenta),
            ),
        ]),
        Line::from(Span::styled(
            commit.subject.clone(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    match app.cache.diff(&commit.hash) {
        None | Some(DiffState::Loading) => {
            lines.push(Line::from(Span::styled(
                "Loading diff…",
                Style::default().fg(Color::DarkGray),
            )));
        }
        Some(DiffState::Failed(msg)) => {
            lines.push(Line::from(Span::styled(
                format!("Failed to load diff: {}", msg),
                Style::default().fg(Color::Red),
            )));
        }
        Some(DiffState::Loaded(diff)) => {
            for line in diff.lines() {
                let style = diff_line_style(line);
                lines.push(Line::from(Span::styled(line.to_string(), style)));
            }
        }
    }

    lines
}

fn diff_line_style(line: &str) -> Style {
    if line.starts_with('+') && !line.starts_with("+++") {
        Style::default().fg(Color::Green)
    } else if line.starts_with('-') && !line.starts_with("---") {
        Style::default().fg(Color::Red)
    } else if line.starts_with("@@") {
        Style::default().fg(Color::Cyan)
    } else if line.starts_with("diff ") || line.starts_with("index ") {
        Style::default().fg(Color::Magenta)
    } else {
        Style::default().fg(Color::Gray)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

pub fn author_color(author: &str) -> Color {
    let hash: u32 = author
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    let colors = [
        Color::Cyan,
        Color::Magenta,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Red,
        Color::LightCyan,
        Color::LightMagenta,
        Color::LightGreen,
        Color::LightYellow,
    ];
    colors[(hash as usize) % colors.len()]
}
