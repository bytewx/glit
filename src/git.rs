use crate::error::AppError;
use std::collections::VecDeque;
use std::process::Command;

#[derive(Clone, Debug)]
pub struct Commit {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    pub author_email: String,
    pub date: String,
    pub subject: String,
    pub graph: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChangedFile {
    pub path: String,
    pub status: String,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlameLine {
    pub line_no: usize,
    pub commit_hash: String,
    pub short_hash: String,
    pub author: String,
    pub author_time: i64,
    pub content: String,
}

pub fn load_commits(max_count: usize) -> Result<Vec<Commit>, AppError> {
    let output = Command::new("git")
        .args([
            "log",
            &format!("--max-count={}", max_count),
            "--graph",
            "--pretty=format:%H%x00%h%x00%an%x00%ae%x00%ad%x00%s%x00",
            "--date=relative",
            "--no-color",
        ])
        .output()
        .map_err(AppError::from)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.contains("not a git repository") || stderr.contains("fatal") {
            return Err(AppError::NotAGitRepo);
        }
        return Err(AppError::GitCommandFailed(stderr));
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    parse_git_log(&raw)
}

pub fn parse_git_log(raw: &str) -> Result<Vec<Commit>, AppError> {
    let mut commits = Vec::new();

    for line in raw.lines() {
        let nul_pos = line.find('\x00');
        if nul_pos.is_none() {
            continue;
        }

        let graph_end = find_graph_end(line);
        let graph = line[..graph_end].to_string();
        let data = &line[graph_end..];

        let fields: Vec<&str> = data.split('\x00').collect();
        if fields.len() < 6 {
            return Err(AppError::MalformedOutput(format!(
                "expected 6 NUL-separated fields, got {}: {:?}",
                fields.len(),
                &fields[..fields.len().min(6)]
            )));
        }

        commits.push(Commit {
            hash: fields[0].to_string(),
            short_hash: fields[1].to_string(),
            author: fields[2].to_string(),
            author_email: fields[3].to_string(),
            date: fields[4].to_string(),
            subject: fields[5].to_string(),
            graph,
        });
    }

    Ok(commits)
}

fn find_graph_end(line: &str) -> usize {
    let mut end = 0;
    for ch in line.chars() {
        if ch.is_ascii_hexdigit() || (!is_graph_char(ch) && ch != ' ') {
            break;
        }
        end += ch.len_utf8();
    }
    end
}

fn is_graph_char(ch: char) -> bool {
    matches!(ch, '*' | '|' | '/' | '\\' | '-' | '_' | '.' | ' ')
        || ('\u{2500}'..='\u{257F}').contains(&ch) // box-drawing unicode
}

pub fn get_diff(hash: &str, max_chars: usize) -> Result<String, AppError> {
    let output = Command::new("git")
        .args(["show", "--stat", "-p", "--no-color", hash])
        .output()
        .map_err(AppError::from)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::GitCommandFailed(stderr));
    }

    let mut s = String::from_utf8_lossy(&output.stdout).to_string();
    if s.chars().count() > max_chars {
        s = s.chars().take(max_chars).collect();
        s.push_str("\n... (diff truncated)");
    }
    Ok(s)
}

pub fn get_branches_for_commit(hash: &str) -> Result<String, AppError> {
    let output = Command::new("git")
        .args(["branch", "--contains", hash, "--format=%(refname:short)"])
        .output()
        .map_err(AppError::from)?;

    if !output.status.success() {
        return Ok(String::new());
    }

    let s = String::from_utf8_lossy(&output.stdout);
    let branches: Vec<&str> = s.lines().filter(|l| !l.is_empty()).take(3).collect();
    Ok(match branches.len() {
        0 => String::new(),
        1 => branches[0].to_string(),
        n => format!("{} +{}", branches[0], n - 1),
    })
}

pub fn changed_files(commit_hash: &str) -> Result<Vec<ChangedFile>, AppError> {
    let output = Command::new("git")
        .args([
            "show",
            "--raw",
            "--numstat",
            "-z",
            "-M",
            "--format=",
            commit_hash,
        ])
        .output()
        .map_err(AppError::from)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::GitCommandFailed(stderr));
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    Ok(parse_changed_files(&raw))
}

pub fn parse_changed_files(raw: &str) -> Vec<ChangedFile> {
    let mut tokens: VecDeque<&str> = raw.split('\x00').collect();
    if tokens.back().is_some_and(|s| s.is_empty()) {
        tokens.pop_back();
    }

    struct RawEntry {
        status: String,
        path: String,
    }

    let mut raw_entries = Vec::new();
    while let Some(front) = tokens.front().copied() {
        if !front.starts_with(':') {
            break;
        }
        let meta = tokens.pop_front().unwrap();
        let status = meta.split_whitespace().last().unwrap_or("").to_string();
        let is_rename_or_copy = status.starts_with('R') || status.starts_with('C');
        let path = if is_rename_or_copy {
            let _old_path = tokens.pop_front().unwrap_or("");
            tokens.pop_front().unwrap_or("").to_string()
        } else {
            tokens.pop_front().unwrap_or("").to_string()
        };
        raw_entries.push(RawEntry { status, path });
    }

    struct NumstatEntry {
        additions: Option<u32>,
        deletions: Option<u32>,
    }

    let mut num_entries = Vec::new();
    while num_entries.len() < raw_entries.len() {
        let Some(tok) = tokens.pop_front() else {
            break;
        };
        let mut parts = tok.splitn(3, '\t');
        let add = parts.next().unwrap_or("0");
        let del = parts.next().unwrap_or("0");
        let path_field = parts.next().unwrap_or("");
        if path_field.is_empty() {
            // Rename/copy numstat record: "add\tdel\t\0old\0new\0"
            let _old_path = tokens.pop_front();
            let _new_path = tokens.pop_front();
        }
        num_entries.push(NumstatEntry {
            additions: add.parse().ok(),
            deletions: del.parse().ok(),
        });
    }

    raw_entries
        .into_iter()
        .zip(num_entries)
        .map(|(r, n)| ChangedFile {
            path: r.path,
            status: r.status,
            additions: n.additions,
            deletions: n.deletions,
        })
        .collect()
}

pub fn blame_file(commit_hash: &str, path: &str) -> Result<Vec<BlameLine>, AppError> {
    let output = Command::new("git")
        .args(["blame", "--line-porcelain", commit_hash, "--", path])
        .output()
        .map_err(AppError::from)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::GitCommandFailed(stderr));
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    Ok(parse_blame(&raw))
}

pub fn parse_blame(raw: &str) -> Vec<BlameLine> {
    let mut result = Vec::new();
    let mut cur_hash: Option<String> = None;
    let mut cur_line_no: usize = 0;
    let mut cur_author = String::new();
    let mut cur_time: i64 = 0;

    for line in raw.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(content) = line.strip_prefix('\t') {
            if let Some(hash) = &cur_hash {
                let short_hash = hash.chars().take(7).collect();
                result.push(BlameLine {
                    line_no: cur_line_no,
                    commit_hash: hash.clone(),
                    short_hash,
                    author: cur_author.clone(),
                    author_time: cur_time,
                    content: content.to_string(),
                });
            }
            continue;
        }

        if line.is_empty() {
            continue;
        }

        let toks: Vec<&str> = line.split(' ').collect();
        let is_header = toks.len() >= 3
            && !toks[0].is_empty()
            && toks[0].chars().all(|c| c.is_ascii_hexdigit())
            && !toks[1].is_empty()
            && toks[1].chars().all(|c| c.is_ascii_digit())
            && !toks[2].is_empty()
            && toks[2].chars().all(|c| c.is_ascii_digit());

        if is_header {
            cur_hash = Some(toks[0].to_string());
            cur_line_no = toks[2].parse().unwrap_or(cur_line_no + 1);
            continue;
        }

        if let Some(v) = line.strip_prefix("author-time ") {
            cur_time = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("author ") {
            cur_author = v.to_string();
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_line(
        hash: &str,
        short: &str,
        author: &str,
        email: &str,
        date: &str,
        subject: &str,
        graph: &str,
    ) -> String {
        format!(
            "{}{}\x00{}\x00{}\x00{}\x00{}\x00{}\x00",
            graph, hash, short, author, email, date, subject
        )
    }

    #[test]
    fn test_parse_basic() {
        let hash = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let line = make_line(
            hash,
            "a1b2c3d",
            "Alice",
            "alice@example.com",
            "2 days ago",
            "fix: something",
            "* ",
        );
        let commits = parse_git_log(&line).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, hash);
        assert_eq!(commits[0].author, "Alice");
        assert_eq!(commits[0].subject, "fix: something");
    }

    #[test]
    fn test_parse_pipe_in_subject() {
        let hash = "b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3";
        let line = make_line(
            hash,
            "b2c3d4e",
            "Bob|Pipe",
            "bob@x.com",
            "1 hour ago",
            "feat: a|b|c pipe test",
            "* ",
        );
        let commits = parse_git_log(&line).unwrap();
        assert_eq!(commits[0].author, "Bob|Pipe");
        assert_eq!(commits[0].subject, "feat: a|b|c pipe test");
    }

    #[test]
    fn test_parse_graph_only_line_ignored() {
        let raw = "|\n| * a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2\x00a1b2c3d\x00Author\x00a@b.com\x00now\x00msg\x00\n|";
        let commits = parse_git_log(raw).unwrap();
        assert_eq!(commits.len(), 1);
    }

    #[test]
    fn test_parse_empty_input() {
        let commits = parse_git_log("").unwrap();
        assert!(commits.is_empty());
    }

    #[test]
    fn test_parse_multiple_commits() {
        let h1 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let h2 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let raw = format!(
            "{}\x00{}\x00A1\x00a@b.com\x00now\x00msg1\x00\n{}\x00{}\x00A2\x00b@c.com\x00then\x00msg2\x00",
            h1,
            &h1[..7],
            h2,
            &h2[..7]
        );
        let commits = parse_git_log(&raw).unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].subject, "msg1");
        assert_eq!(commits[1].subject, "msg2");
    }

    #[test]
    fn test_parse_changed_files_basic() {
        let raw = ":100644 100644 aaa bbb M\x00src/main.rs\x00:000000 100644 000 ccc A\x00src/new.rs\x00\
                    3\t1\tsrc/main.rs\x005\t0\tsrc/new.rs\x00";
        let files = parse_changed_files(raw);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[0].status, "M");
        assert_eq!(files[0].additions, Some(3));
        assert_eq!(files[0].deletions, Some(1));
        assert_eq!(files[1].path, "src/new.rs");
        assert_eq!(files[1].status, "A");
        assert_eq!(files[1].additions, Some(5));
        assert_eq!(files[1].deletions, Some(0));
    }

    #[test]
    fn test_parse_changed_files_rename() {
        let raw = ":100644 100644 aaa bbb R097\x00old.rs\x00new.rs\x00\
                    1\t0\t\x00old.rs\x00new.rs\x00";
        let files = parse_changed_files(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "new.rs");
        assert_eq!(files[0].status, "R097");
        assert_eq!(files[0].additions, Some(1));
        assert_eq!(files[0].deletions, Some(0));
    }

    #[test]
    fn test_parse_changed_files_deleted() {
        let raw = ":100644 000000 aaa 000 D\x00gone.rs\x00\
                    0\t42\tgone.rs\x00";
        let files = parse_changed_files(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, "D");
        assert_eq!(files[0].deletions, Some(42));
    }

    #[test]
    fn test_parse_changed_files_binary() {
        let raw = ":100644 100644 aaa bbb M\x00image.png\x00-\t-\timage.png\x00";
        let files = parse_changed_files(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].additions, None);
        assert_eq!(files[0].deletions, None);
    }

    #[test]
    fn test_parse_changed_files_empty() {
        let files = parse_changed_files("");
        assert!(files.is_empty());
    }

    fn blame_block(
        hash: &str,
        orig_line: usize,
        final_line: usize,
        group: Option<usize>,
        author: &str,
        author_time: i64,
        content: &str,
    ) -> String {
        let header = match group {
            Some(g) => format!("{} {} {} {}", hash, orig_line, final_line, g),
            None => format!("{} {} {}", hash, orig_line, final_line),
        };
        format!(
            "{}\nauthor {}\nauthor-mail <{}@example.com>\nauthor-time {}\nauthor-tz +0000\ncommitter {}\ncommitter-mail <{}@example.com>\ncommitter-time {}\ncommitter-tz +0000\nsummary msg\nfilename file.rs\n\t{}\n",
            header, author, author, author_time, author, author, author_time, content
        )
    }

    #[test]
    fn test_parse_blame_basic() {
        let hash = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let raw = blame_block(hash, 1, 1, Some(1), "Alice", 1700000000, "fn main() {}");
        let lines = parse_blame(&raw);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line_no, 1);
        assert_eq!(lines[0].commit_hash, hash);
        assert_eq!(lines[0].short_hash, hash[..7]);
        assert_eq!(lines[0].author, "Alice");
        assert_eq!(lines[0].author_time, 1700000000);
        assert_eq!(lines[0].content, "fn main() {}");
    }

    #[test]
    fn test_parse_blame_multiple_lines_same_group() {
        let hash = "b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3";
        let mut raw = blame_block(hash, 1, 1, Some(2), "Bob", 1600000000, "line one");
        raw.push_str(&blame_block(
            hash, 2, 2, None, "Bob", 1600000000, "line two",
        ));
        let lines = parse_blame(&raw);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].content, "line one");
        assert_eq!(lines[1].content, "line two");
        assert_eq!(lines[1].line_no, 2);
    }

    #[test]
    fn test_parse_blame_unicode_content_and_author() {
        let hash = "c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4";
        let raw = blame_block(
            hash,
            1,
            1,
            Some(1),
            "bytewx",
            1650000000,
            "let s = \"unicode support\";",
        );
        let lines = parse_blame(&raw);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].author, "bytewx");
        assert_eq!(lines[0].content, "let s = \"unicode support\";");
    }

    #[test]
    fn test_parse_blame_unusual_author_names() {
        let hash = "d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5";
        let raw = blame_block(
            hash,
            1,
            1,
            Some(1),
            "Anne-Marie O'Neil-Smith Jr.",
            1690000000,
            "content",
        );
        let lines = parse_blame(&raw);
        assert_eq!(lines[0].author, "Anne-Marie O'Neil-Smith Jr.");
    }

    #[test]
    fn test_parse_blame_content_resembling_header() {
        let hash = "e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6";
        let raw = blame_block(
            hash,
            1,
            1,
            Some(1),
            "Alice",
            1700000000,
            "author 12 34 not a real header",
        );
        let lines = parse_blame(&raw);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].content, "author 12 34 not a real header");
        assert_eq!(lines[0].author, "Alice");
    }

    #[test]
    fn test_parse_blame_empty_line_content() {
        let hash = "f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1";
        let raw = blame_block(hash, 1, 1, Some(1), "Alice", 1700000000, "");
        let lines = parse_blame(&raw);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].content, "");
    }

    #[test]
    fn test_parse_blame_empty_input() {
        let lines = parse_blame("");
        assert!(lines.is_empty());
    }
}
