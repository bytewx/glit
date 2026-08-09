pub struct Config {
    pub max_commits: usize,
    pub diff_char_limit: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_commits: 200,
            diff_char_limit: 8000,
        }
    }
}
