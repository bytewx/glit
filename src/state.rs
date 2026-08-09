use crate::git::{BlameLine, ChangedFile, Commit};
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use ratatui::widgets::ListState;
use std::collections::HashMap;
use std::sync::mpsc;

pub enum DiffState {
    Loading,
    Loaded(String),
    Failed(String),
}

pub enum BranchState {
    Loading,
    Loaded(String),
}

pub enum ChangedFilesState {
    Loading,
    Loaded(Vec<ChangedFile>),
    Failed(String),
}

pub enum BlameCacheState {
    Loading,
    Loaded(Vec<BlameLine>),
    Failed(String),
}

pub enum LoadMsg {
    Diff {
        hash: String,
        result: Result<String, String>,
    },
    Branch {
        hash: String,
        result: String,
    },
    ChangedFiles {
        hash: String,
        result: Result<Vec<ChangedFile>, String>,
    },
    Blame {
        hash: String,
        path: String,
        result: Result<Vec<BlameLine>, String>,
    },
}

pub struct Cache {
    pub diffs: HashMap<String, DiffState>,
    pub branches: HashMap<String, BranchState>,
    pub changed_files: HashMap<String, ChangedFilesState>,
    pub blame: HashMap<(String, String), BlameCacheState>,
}

impl Cache {
    pub fn new() -> Self {
        Cache {
            diffs: HashMap::new(),
            branches: HashMap::new(),
            changed_files: HashMap::new(),
            blame: HashMap::new(),
        }
    }

    pub fn diff(&self, hash: &str) -> Option<&DiffState> {
        self.diffs.get(hash)
    }

    pub fn branch(&self, hash: &str) -> Option<&BranchState> {
        self.branches.get(hash)
    }

    pub fn changed_files_for(&self, hash: &str) -> Option<&ChangedFilesState> {
        self.changed_files.get(hash)
    }

    pub fn blame_for(&self, hash: &str, path: &str) -> Option<&BlameCacheState> {
        self.blame.get(&(hash.to_string(), path.to_string()))
    }

    pub fn insert_diff(&mut self, hash: String, state: DiffState) {
        self.diffs.insert(hash, state);
    }

    pub fn insert_branch(&mut self, hash: String, state: BranchState) {
        self.branches.insert(hash, state);
    }

    pub fn insert_changed_files(&mut self, hash: String, state: ChangedFilesState) {
        self.changed_files.insert(hash, state);
    }

    pub fn insert_blame(&mut self, hash: String, path: String, state: BlameCacheState) {
        self.blame.insert((hash, path), state);
    }
}

#[derive(PartialEq)]
pub enum Focus {
    List,
    Files,
    Preview,
}

pub struct BlameView {
    pub commit_hash: String,
    pub file_path: String,
    pub selected_line: usize,
    pub scroll_offset: u16,
}

pub struct App {
    pub commits: Vec<Commit>,
    pub filtered: Vec<usize>,
    pub diff_char_limit: usize,
    pub list_state: ListState,
    pub files_list_state: ListState,
    pub query: String,
    pub matcher: SkimMatcherV2,
    pub diff_scroll: u16,
    pub status: String,
    pub focus: Focus,
    pub cache: Cache,
    pub blame_view: Option<BlameView>,
    pub tx: mpsc::Sender<LoadMsg>,
    pub rx: mpsc::Receiver<LoadMsg>,
}

impl App {
    pub fn new(commits: Vec<Commit>, diff_char_limit: usize) -> Self {
        let filtered: Vec<usize> = (0..commits.len()).collect();
        let mut list_state = ListState::default();
        if !filtered.is_empty() {
            list_state.select(Some(0));
        }
        let mut files_list_state = ListState::default();
        files_list_state.select(Some(0));
        let (tx, rx) = mpsc::channel();
        App {
            commits,
            filtered,
            diff_char_limit,
            list_state,
            files_list_state,
            query: String::new(),
            matcher: SkimMatcherV2::default(),
            diff_scroll: 0,
            status: String::new(),
            focus: Focus::List,
            cache: Cache::new(),
            blame_view: None,
            tx,
            rx,
        }
    }

    pub fn update_filter(&mut self) {
        if self.query.is_empty() {
            self.filtered = (0..self.commits.len()).collect();
        } else if self.query.starts_with('@') {
            let author_query = self.query[1..].to_lowercase();
            self.filtered = self
                .commits
                .iter()
                .enumerate()
                .filter(|(_, c)| c.author.to_lowercase().contains(&author_query))
                .map(|(i, _)| i)
                .collect();
        } else {
            self.filtered = self
                .commits
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    let haystack = format!("{} {}", c.subject, c.author);
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
        self.diff_scroll = 0;
        self.on_commit_changed();
    }

    pub fn selected_commit(&self) -> Option<&Commit> {
        let sel = self.list_state.selected()?;
        let idx = self.filtered.get(sel)?;
        self.commits.get(*idx)
    }

    pub fn selected_hash(&self) -> Option<String> {
        self.selected_commit().map(|c| c.hash.clone())
    }

    pub fn move_up(&mut self) {
        if let Some(sel) = self.list_state.selected()
            && sel > 0
        {
            self.list_state.select(Some(sel - 1));
        }
        self.diff_scroll = 0;
        self.on_commit_changed();
    }

    pub fn move_down(&mut self) {
        if let Some(sel) = self.list_state.selected()
            && sel + 1 < self.filtered.len()
        {
            self.list_state.select(Some(sel + 1));
        }
        self.diff_scroll = 0;
        self.on_commit_changed();
    }

    fn on_commit_changed(&mut self) {
        self.files_list_state.select(Some(0));
        self.blame_view = None;
    }

    pub fn scroll_diff_down(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_add(3);
    }

    pub fn scroll_diff_up(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_sub(3);
    }

    pub fn changed_files_len(&self) -> usize {
        let Some(hash) = self.selected_hash() else {
            return 0;
        };
        match self.cache.changed_files_for(&hash) {
            Some(ChangedFilesState::Loaded(files)) => files.len(),
            _ => 0,
        }
    }

    pub fn selected_changed_file(&self) -> Option<&ChangedFile> {
        let hash = self.selected_hash()?;
        let idx = self.files_list_state.selected()?;
        match self.cache.changed_files_for(&hash) {
            Some(ChangedFilesState::Loaded(files)) => files.get(idx),
            _ => None,
        }
    }

    pub fn move_file_down(&mut self) {
        let len = self.changed_files_len();
        if len == 0 {
            return;
        }
        let sel = self.files_list_state.selected().unwrap_or(0);
        if sel + 1 < len {
            self.files_list_state.select(Some(sel + 1));
        } else {
            self.files_list_state.select(Some(sel));
        }
    }

    pub fn move_file_up(&mut self) {
        let sel = self.files_list_state.selected().unwrap_or(0);
        if sel > 0 {
            self.files_list_state.select(Some(sel - 1));
        } else {
            self.files_list_state.select(Some(0));
        }
    }

    pub fn open_blame(&mut self, commit_hash: String, file_path: String) {
        self.blame_view = Some(BlameView {
            commit_hash,
            file_path,
            selected_line: 0,
            scroll_offset: 0,
        });
        self.focus = Focus::Preview;
    }

    pub fn close_blame(&mut self) {
        self.blame_view = None;
    }

    pub fn blame_line_count(&self) -> usize {
        let Some(bv) = &self.blame_view else {
            return 0;
        };
        match self.cache.blame_for(&bv.commit_hash, &bv.file_path) {
            Some(BlameCacheState::Loaded(lines)) => lines.len(),
            _ => 0,
        }
    }

    pub fn blame_move_down(&mut self) {
        let total = self.blame_line_count();
        if let Some(bv) = &mut self.blame_view
            && total > 0
            && bv.selected_line + 1 < total
        {
            bv.selected_line += 1;
        }
    }

    pub fn blame_move_up(&mut self) {
        if let Some(bv) = &mut self.blame_view
            && bv.selected_line > 0
        {
            bv.selected_line -= 1;
        }
    }
}
