use anyhow::{Context, Result, ensure};
use bincode::Options;
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Serialize)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    #[serde(skip)]
    pub identity: PathBuf,
    #[serde(skip)]
    pub git_root: Option<PathBuf>,
    pub branch: Option<String>,
    pub score: f64,
    pub last_accessed: Option<u64>,
}

#[derive(Deserialize)]
struct LegacyDir {
    path: String,
    _rank: f64,
    last_accessed: u64,
}

fn timestamps(data: &Path) -> Result<HashMap<String, u64>> {
    if data.join("db.txt").exists() {
        let text = fs::read_to_string(data.join("db.txt"))?;
        let mut result = HashMap::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let mut fields = line.splitn(3, '\t');
            let timestamp = fields.next().context("missing timestamp")?.trim().parse()?;
            fields.next().context("missing rank")?;
            let path = fields
                .next()
                .context("unsupported zoxide database format")?;
            result.insert(path.to_owned(), timestamp);
        }
        Ok(result)
    } else if data.join("db.zo").exists() {
        let bytes = fs::read(data.join("db.zo"))?;
        ensure!(
            bytes.len() >= 4 && bytes[..4] == 3_u32.to_le_bytes(),
            "unsupported zoxide database version"
        );
        let dirs: Vec<LegacyDir> = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(32 << 20)
            .deserialize(&bytes[4..])?;
        Ok(dirs
            .into_iter()
            .map(|d| (d.path, d.last_accessed))
            .collect())
    } else {
        Ok(HashMap::new())
    }
}

pub(crate) fn repository(path: &Path) -> Option<(PathBuf, PathBuf)> {
    let path = fs::canonicalize(path).ok()?;
    for root in path.ancestors() {
        let marker = root.join(".git");
        let git_dir = if marker.is_dir() {
            marker
        } else if marker.is_file() {
            let text = fs::read_to_string(marker).ok()?;
            let dir = PathBuf::from(text.trim_end().strip_prefix("gitdir: ")?);
            if dir.is_absolute() {
                dir
            } else {
                root.join(dir)
            }
        } else {
            continue;
        };
        if git_dir.join("HEAD").is_file() {
            return Some((root.to_path_buf(), git_dir));
        }
    }
    None
}

fn branch(git_dir: &Path) -> Option<String> {
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref: ") {
        Some(
            reference
                .strip_prefix("refs/heads/")
                .unwrap_or(reference)
                .to_owned(),
        )
    } else if head.len() >= 7 && head.bytes().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("detached:{}", &head[..7]))
    } else {
        None
    }
}

pub fn load() -> Result<Vec<Project>> {
    let data = std::env::var_os("_ZO_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::data_local_dir().map(|p| p.join("zoxide")))
        .context("cannot locate zoxide data; set _ZO_DATA_DIR")?;
    let times = match timestamps(&data) {
        Ok(times) => times,
        Err(error) => {
            eprintln!("zurf: last-access metadata unavailable: {error}");
            HashMap::new()
        }
    };
    let output = Command::new("zoxide")
        .args(["query", "--list", "--score"])
        .output()
        .context("zoxide is required; install it and enable its shell integration")?;
    ensure!(
        output.status.success(),
        "zoxide query failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut projects = Vec::new();
    for line in String::from_utf8(output.stdout)?.lines() {
        let line = line.trim_start();
        let (score, path) = line
            .split_once(char::is_whitespace)
            .context("unexpected zoxide query output")?;
        let score: f64 = score.parse()?;
        ensure!(score.is_finite(), "invalid zoxide score");
        let original = PathBuf::from(path.trim_start());
        if !original.is_dir() {
            continue;
        }
        let repo = repository(&original);
        let visited = times.get(&original.to_string_lossy().into_owned()).copied();
        let branch = repo.as_ref().and_then(|(_, git_dir)| branch(git_dir));
        let name = original
            .file_name()
            .unwrap_or(original.as_os_str())
            .to_string_lossy()
            .into_owned();
        projects.push(Project {
            name,
            identity: fs::canonicalize(&original).unwrap_or_else(|_| original.clone()),
            git_root: repo.as_ref().map(|(root, _)| root.clone()),
            path: original,
            branch,
            score,
            last_accessed: visited,
        });
    }
    for path in crate::preferences::Preferences::load()?.pins {
        if !path.is_dir() || projects.iter().any(|p| p.identity == path) {
            continue;
        }
        let repo = repository(&path);
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        let branch = repo.as_ref().and_then(|(_, git_dir)| branch(git_dir));
        projects.push(Project {
            name,
            identity: fs::canonicalize(&path).unwrap_or_else(|_| path.clone()),
            git_root: repo.as_ref().map(|(root, _)| root.clone()),
            path,
            branch,
            score: 0.0,
            last_accessed: None,
        });
    }
    Ok(projects)
}

pub fn matching(projects: &[Project], query: &str) -> Vec<usize> {
    if query.trim().is_empty() {
        return (0..projects.len()).collect();
    }
    let matcher = SkimMatcherV2::default().ignore_case();
    let mut matches: Vec<_> = projects
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let mut score = matcher
                .fuzzy_match(&p.name, query)
                .into_iter()
                .chain(
                    p.branch
                        .as_deref()
                        .and_then(|b| matcher.fuzzy_match(b, query)),
                )
                .max();
            if query.contains('/') || query.starts_with('~') {
                score = score
                    .into_iter()
                    .chain(matcher.fuzzy_match(&display_path(&p.path), query))
                    .max();
            }
            score.map(|score| (i, score))
        })
        .collect();
    matches.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    matches.into_iter().map(|(i, _)| i).collect()
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn relative_time(time: Option<u64>, now: u64) -> String {
    let Some(time) = time else {
        return "unknown".into();
    };
    let age = now.saturating_sub(time);
    let (amount, unit) = match age {
        0..60 => return "just now".into(),
        60..3600 => (age / 60, "minute"),
        3600..86400 => (age / 3600, "hour"),
        86400..172800 => return "yesterday".into(),
        172800..604800 => (age / 86400, "day"),
        604800..2592000 => (age / 604800, "week"),
        2592000..31536000 => (age / 2592000, "month"),
        _ => (age / 31536000, "year"),
    };
    format!("{amount} {unit}{} ago", if amount == 1 { "" } else { "s" })
}

pub fn display_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(relative) = path.strip_prefix(home) {
            return if relative.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", relative.display())
            };
        }
    }
    path.display().to_string()
}
