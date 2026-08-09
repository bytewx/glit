use std::fmt;

#[derive(Debug)]
pub enum AppError {
    NotAGitRepo,
    GitNotFound,
    GitCommandFailed(String),
    MalformedOutput(String),
    Io(std::io::Error),
    Terminal(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NotAGitRepo => write!(f, "Not inside a git repository"),
            AppError::GitNotFound => write!(f, "git command not found — is git installed?"),
            AppError::GitCommandFailed(msg) => write!(f, "git command failed: {}", msg),
            AppError::MalformedOutput(msg) => write!(f, "malformed git output: {}", msg),
            AppError::Io(e) => write!(f, "IO error: {}", e),
            AppError::Terminal(msg) => write!(f, "terminal error: {}", msg),
        }
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::GitNotFound
        } else {
            AppError::Io(e)
        }
    }
}
