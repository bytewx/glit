use arboard::Clipboard;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::{error::Error, io, process::Command};

#[derive(PartialEq)]
enum Focus {
    List,
    Diff,
}

struct Commit {
    id: String,
    message: String,
    author: String,
    date: String,
    diff: String,
    graph: String,
}

struct App {
    commits: Vec<Commit>,
    filtered: Vec<usize>,
    list_state: ListState,
    query: String,
    matcher: SkimMatcherV2,
    diff_scroll: u16,
    status: String,
    focus: Focus,
}

impl App {
    fn new(commits: Vec<Commit>) -> Self {
        let filtered: Vec<usize> = (0..commits.len()).collect();
        let mut list_state = ListState::default();
        if !filtered.is_empty() {
            list_state.select(Some(0));
        }
        App { commits, filtered, list_state, query: String::new(), matcher: SkimMatcherV2::default(), diff_scroll: 0, status: String::new(), focus: Focus::List }
    }

    fn update_filter(&mut self) {
        if self.query.is_empty() {
            self.filtered = (0..self.commits.len()).collect();
        } else if self.query.starts_with('@') {
            let author_query = self.query[1..].to_lowercase();
            self.filtered = self.commits.iter().enumerate()
                .filter(|(_, c)| c.author.to_lowercase().contains(&author_query))
                .map(|(i, _)| i)
                .collect();
        } else {
            self.filtered = self.commits.iter().enumerate()
                .filter(|(_, c)| {
                    let haystack = format!("{} {}", c.message, c.author);
                    self.matcher.fuzzy_match(&haystack, &self.query).is_some()
                })
                .map(|(i, _)| i)
                .collect();
        }
        if self.filtered.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
    }

    fn selected_commit(&self) -> Option<&Commit> {
        let sel = self.list_state.selected()?;
        let idx = self.filtered.get(sel)?;
        self.commits.get(*idx)
    }

    fn move_up(&mut self) {
        if let Some(sel) = self.list_state.selected() {
            if sel > 0 { self.list_state.select(Some(sel - 1)); }
        }
    }

    fn move_down(&mut self) {
        if let Some(sel) = self.list_state.selected() {
            if sel + 1 < self.filtered.len() { self.list_state.select(Some(sel + 1)); }
        }
    }

    fn scroll_diff_down(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_add(3);
    }

    fn scroll_diff_up(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_sub(3);
    }

    fn reset_diff_scroll(&mut self) {
        self.diff_scroll = 0;
    }
}

fn load_commits() -> Result<Vec<Commit>, Box<dyn Error>> {
    let output = Command::new("git")
        .args([
            "log",
            "--max-count=200",
            "--graph",
            "--pretty=format:COMMIT:%H|%s|%an|%ar",
            "--no-color",
        ])
        .output()?;

    if !output.status.success() {
        return Err("Not a git repository or git not found".into());
    }

    let log = String::from_utf8_lossy(&output.stdout);
    let mut commits = Vec::new();
    let mut current_graph = String::new();

    for line in log.lines() {
        if let Some(data) = line.find("COMMIT:") {
            let graph_part = line[..data].to_string();
            let data_part = &line[data + 7..];
            let parts: Vec<&str> = data_part.splitn(4, '|').collect();
            if parts.len() < 4 { continue; }

            let full_hash = parts[0].to_string();
            let id = full_hash[..7.min(full_hash.len())].to_string();
            let message = parts[1].to_string();
            let author = parts[2].to_string();
            let date = parts[3].to_string();
            let diff = get_diff(&full_hash);

            commits.push(Commit {
                id, message, author, date, diff,
                graph: graph_part,
            });
            current_graph = String::new();
        } else {
            current_graph = line.to_string();
        }
    }

    Ok(commits)
}

fn get_diff(hash: &str) -> String {
    let output = Command::new("git")
        .args(["show", "--stat", "-p", "--no-color", hash])
        .output();

    match output {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).to_string();
            if s.chars().count() > 6000 {
                s = s.chars().take(6000).collect();
                s.push_str("\n... (diff truncated)");
            }
            s
        }
        Err(_) => "(could not load diff)".to_string(),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let commits = load_commits().unwrap_or_else(|e| {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    });

    if commits.is_empty() {
        eprintln!("No commits found.");
        return Ok(());
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(commits);
    let result = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;

    if let Err(e) = result { eprintln!("Error: {}", e); }
    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), Box<dyn Error>> {
    loop {
        terminal.draw(|f| ui(f, app))?;
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc => return Ok(()),
                KeyCode::PageDown => app.scroll_diff_down(),
                KeyCode::PageUp => app.scroll_diff_up(),
                KeyCode::Tab => {
                    app.focus = if app.focus == Focus::List { Focus::Diff } else { Focus::List };
                }
                KeyCode::Up => {
                    if app.focus == Focus::List {
                        app.move_up(); app.reset_diff_scroll();
                    } else {
                        app.scroll_diff_up();
                    }
                }
                KeyCode::Down => {
                    if app.focus == Focus::List {
                        app.move_down(); app.reset_diff_scroll();
                    } else {
                        app.scroll_diff_down();
                    }
                }
                KeyCode::Char(c) => { app.query.push(c); app.update_filter(); }
                KeyCode::Backspace => { app.query.pop(); app.update_filter(); }
                KeyCode::Enter => {
                    if let Some(commit) = app.selected_commit() {
                        let hash = commit.id.clone();


                        let copied = copy_to_clipboard(&hash);
                        app.status = if copied {
                            format!("Copied: {}", hash)
                        } else {
                            "Failed to copy".to_string()
                        };
                    }
                }
                _ => {}
            }
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let size = f.size();

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)].as_ref())
        .split(size);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ].as_ref())
        .split(main_chunks[0]);

    let search = Paragraph::new(format!(" {}_", app.query))
        .block(Block::default().borders(Borders::ALL)
            .title(" Search — type to filter, @name for author, Tab focus, ESC quit ")
            .border_style(Style::default().fg(Color::Yellow)));
    f.render_widget(search, left_chunks[0]);

    let status_text = if app.status.is_empty() {
        " Enter — copy hash".to_string()
    } else {
        format!(" ✓ {}", app.status)
    };
    let status = Paragraph::new(status_text)
        .style(Style::default().fg(if app.status.is_empty() { Color::DarkGray } else { Color::Green }));
    f.render_widget(status, left_chunks[1]);

    let items: Vec<ListItem> = app.filtered.iter().map(|&i| {
        let c = &app.commits[i];
        let color = author_color(&c.author);

        let graph_span = Span::styled(
            c.graph.clone(),
            Style::default().fg(Color::DarkGray),
        );

        let line = Line::from(vec![
            graph_span,
            Span::styled(format!("{} ", c.id), Style::default().fg(Color::Yellow)),
            Span::styled(truncate(&c.message, 18), Style::default().fg(Color::White)),
            Span::styled(format!(" · {}", truncate(&c.author, 10)), Style::default().fg(color)),
        ]);
        ListItem::new(line)
    }).collect();

    let list_border_color = if app.focus == Focus::List { Color::Yellow } else { Color::Cyan };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL)
            .title(format!(" {} commits ", app.filtered.len()))
            .border_style(Style::default().fg(list_border_color)))
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, left_chunks[2], &mut app.list_state);

    let diff_lines: Vec<Line> = if let Some(commit) = app.selected_commit() {
        let branch = get_branch_for_commit(&commit.id);
        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    format!("commit {}   by ", commit.id),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    commit.author.clone(),
                    Style::default().fg(author_color(&commit.author)).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled(format!("{}  ", commit.date), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("⎇ {}", branch), Style::default().fg(Color::Magenta)),
            ]),
            Line::from(Span::styled(
                commit.message.clone(),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];
        for line in commit.diff.lines() {
            let style = if line.starts_with('+') && !line.starts_with("+++") {
                Style::default().fg(Color::Green)
            } else if line.starts_with('-') && !line.starts_with("---") {
                Style::default().fg(Color::Red)
            } else if line.starts_with("@@") {
                Style::default().fg(Color::Cyan)
            } else if line.starts_with("diff ") || line.starts_with("index ") {
                Style::default().fg(Color::Magenta)
            } else {
                Style::default().fg(Color::Gray)
            };
            lines.push(Line::from(Span::styled(line.to_string(), style)));
        }
        lines
    } else {
        vec![Line::from("No commit selected")]
    };

    let diff_border_color = if app.focus == Focus::Diff { Color::Yellow } else { Color::Cyan };
    let diff = Paragraph::new(diff_lines)
        .block(Block::default().borders(Borders::ALL)
            .title(" Diff (Tab to focus, ↑↓ to scroll) ")
            .border_style(Style::default().fg(diff_border_color)))
        .wrap(Wrap { trim: false })
        .scroll((app.diff_scroll, 0));

    f.render_widget(diff, main_chunks[1]);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

fn copy_to_clipboard(text: &str) -> bool {
    if let Ok(mut child) = Command::new("clip.exe").stdin(std::process::Stdio::piped()).spawn() {
        if let Some(stdin) = child.stdin.take() {
            use std::io::Write;
            let mut stdin = stdin;
            let _ = stdin.write_all(text.as_bytes());
        }
        return child.wait().map(|s| s.success()).unwrap_or(false);
    }
    if let Ok(mut clipboard) = Clipboard::new() {
        return clipboard.set_text(text.to_string()).is_ok();
    }
    false
}

fn author_color(author: &str) -> Color {
    let hash: u32 = author.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
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

fn get_branch_for_commit(hash: &str) -> String {
    let output = Command::new("git")
        .args(["branch", "--contains", hash, "--format=%(refname:short)"])
        .output();

    match output {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout);
            let branches: Vec<&str> = s.lines().take(2).collect();
            if branches.is_empty() {
                String::new()
            } else if branches.len() == 1 {
                branches[0].to_string()
            } else {
                format!("{} +{}", branches[0], branches.len() - 1)
            }
        }
        Err(_) => String::new(),
    }
}