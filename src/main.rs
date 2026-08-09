mod config;
mod error;
mod git;
mod state;
mod ui;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;

use config::Config;
use error::AppError;
use state::{App, BlameCacheState, BranchState, ChangedFilesState, DiffState, Focus, LoadMsg};

fn main() {
    if let Err(e) = run() {
        eprintln!("glit: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), AppError> {
    let cfg = Config::default();

    let commits = git::load_commits(cfg.max_commits)?;
    if commits.is_empty() {
        eprintln!("No commits found.");
        return Ok(());
    }

    enable_raw_mode().map_err(|e| AppError::Terminal(e.to_string()))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .map_err(|e| AppError::Terminal(e.to_string()))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| AppError::Terminal(e.to_string()))?;

    let mut app = App::new(commits);

    trigger_load(&mut app);

    let result = run_app(&mut terminal, &mut app);

    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );

    result
}

fn trigger_load(app: &mut App) {
    let Some(hash) = app.selected_hash() else {
        return;
    };

    if !app.cache.diffs.contains_key(&hash) {
        app.cache.insert_diff(hash.clone(), DiffState::Loading);
        let tx = app.tx.clone();
        let h = hash.clone();
        std::thread::spawn(move || {
            let result = git::get_diff(&h).map_err(|e| e.to_string());
            let _ = tx.send(LoadMsg::Diff { hash: h, result });
        });
    }

    if !app.cache.branches.contains_key(&hash) {
        app.cache.insert_branch(hash.clone(), BranchState::Loading);
        let tx = app.tx.clone();
        let h = hash.clone();
        std::thread::spawn(move || {
            let result = git::get_branches_for_commit(&h).unwrap_or_default();
            let _ = tx.send(LoadMsg::Branch { hash: h, result });
        });
    }

    if !app.cache.changed_files.contains_key(&hash) {
        app.cache
            .insert_changed_files(hash.clone(), ChangedFilesState::Loading);
        let tx = app.tx.clone();
        let h = hash.clone();
        std::thread::spawn(move || {
            let result = git::changed_files(&h).map_err(|e| e.to_string());
            let _ = tx.send(LoadMsg::ChangedFiles { hash: h, result });
        });
    }
}

fn trigger_blame_load(app: &mut App, hash: &str, path: &str) {
    if app.cache.blame_for(hash, path).is_some() {
        return;
    }
    app.cache
        .insert_blame(hash.to_string(), path.to_string(), BlameCacheState::Loading);
    let tx = app.tx.clone();
    let h = hash.to_string();
    let p = path.to_string();
    std::thread::spawn(move || {
        let result = git::blame_file(&h, &p).map_err(|e| e.to_string());
        let _ = tx.send(LoadMsg::Blame {
            hash: h,
            path: p,
            result,
        });
    });
}

fn drain_channel(app: &mut App) {
    while let Ok(msg) = app.rx.try_recv() {
        match msg {
            LoadMsg::Diff { hash, result } => {
                let state = match result {
                    Ok(d) => DiffState::Loaded(d),
                    Err(e) => DiffState::Failed(e),
                };
                app.cache.insert_diff(hash, state);
            }
            LoadMsg::Branch { hash, result } => {
                app.cache.insert_branch(hash, BranchState::Loaded(result));
            }
            LoadMsg::ChangedFiles { hash, result } => {
                let state = match result {
                    Ok(files) => ChangedFilesState::Loaded(files),
                    Err(e) => ChangedFilesState::Failed(e),
                };
                app.cache.insert_changed_files(hash, state);
            }
            LoadMsg::Blame { hash, path, result } => {
                let state = match result {
                    Ok(lines) => BlameCacheState::Loaded(lines),
                    Err(e) => BlameCacheState::Failed(e),
                };
                app.cache.insert_blame(hash, path, state);
            }
        }
    }
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<(), AppError> {
    loop {
        drain_channel(app);

        terminal
            .draw(|f| ui::draw(f, app))
            .map_err(|e| AppError::Terminal(e.to_string()))?;

        if !event::poll(std::time::Duration::from_millis(50))
            .map_err(|e| AppError::Terminal(e.to_string()))?
        {
            continue;
        }

        if let Event::Key(key) = event::read().map_err(|e| AppError::Terminal(e.to_string()))? {
            match key.code {
                KeyCode::Esc => {
                    if app.blame_view.is_some() {
                        app.close_blame();
                    } else {
                        return Ok(());
                    }
                }

                KeyCode::Tab => {
                    app.focus = match app.focus {
                        Focus::List => Focus::Files,
                        Focus::Files => Focus::Preview,
                        Focus::Preview => Focus::List,
                    };
                }

                KeyCode::Up => match app.focus {
                    Focus::List => {
                        app.move_up();
                        trigger_load(app);
                    }
                    Focus::Files => app.move_file_up(),
                    Focus::Preview => {
                        if app.blame_view.is_some() {
                            app.blame_move_up();
                        } else {
                            app.scroll_diff_up();
                        }
                    }
                },

                KeyCode::Down => match app.focus {
                    Focus::List => {
                        app.move_down();
                        trigger_load(app);
                    }
                    Focus::Files => app.move_file_down(),
                    Focus::Preview => {
                        if app.blame_view.is_some() {
                            app.blame_move_down();
                        } else {
                            app.scroll_diff_down();
                        }
                    }
                },

                KeyCode::PageDown => {
                    if app.focus == Focus::Preview && app.blame_view.is_none() {
                        app.scroll_diff_down();
                    }
                }
                KeyCode::PageUp => {
                    if app.focus == Focus::Preview && app.blame_view.is_none() {
                        app.scroll_diff_up();
                    }
                }

                KeyCode::Char(c) => match app.focus {
                    Focus::List => {
                        app.query.push(c);
                        app.update_filter();
                        trigger_load(app);
                    }
                    Focus::Files => match c {
                        'j' => app.move_file_down(),
                        'k' => app.move_file_up(),
                        'b' => {
                            if let (Some(hash), Some(path)) =
                                (app.selected_hash(), app.selected_changed_file().map(|f| f.path.clone()))
                            {
                                app.open_blame(hash.clone(), path.clone());
                                trigger_blame_load(app, &hash, &path);
                            }
                        }
                        _ => {}
                    },
                    Focus::Preview => match c {
                        'j' => {
                            if app.blame_view.is_some() {
                                app.blame_move_down();
                            } else {
                                app.scroll_diff_down();
                            }
                        }
                        'k' => {
                            if app.blame_view.is_some() {
                                app.blame_move_up();
                            } else {
                                app.scroll_diff_up();
                            }
                        }
                        _ => {}
                    },
                },

                KeyCode::Backspace => {
                    if app.focus == Focus::List {
                        app.query.pop();
                        app.update_filter();
                        trigger_load(app);
                    }
                }

                KeyCode::Enter => {
                    if let Some(hash) = app.selected_hash() {
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

fn copy_to_clipboard(text: &str) -> bool {
    if let Ok(mut child) = std::process::Command::new("clip.exe")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(text.as_bytes());
        }
        return child.wait().map(|s| s.success()).unwrap_or(false);
    }
    if let Ok(mut cb) = arboard::Clipboard::new() {
        return cb.set_text(text.to_string()).is_ok();
    }
    false
}