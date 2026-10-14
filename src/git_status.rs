use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub struct GitWorker {
    pub requests: SyncSender<PathBuf>,
    pub results: Receiver<(PathBuf, Option<GitStatus>)>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for GitWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn start() -> GitWorker {
    let (requests, pending) = mpsc::sync_channel::<PathBuf>(16);
    let (completed, results) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let thread = thread::spawn(move || {
        while !worker_stop.load(Ordering::Relaxed) {
            match pending.recv_timeout(Duration::from_millis(20)) {
                Ok(path) => {
                    let status = query(&path, &worker_stop);
                    if completed.send((path, status)).is_err() {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    GitWorker {
        requests,
        results,
        stop,
        thread: Some(thread),
    }
}

fn query(path: &std::path::Path, stop: &AtomicBool) -> Option<GitStatus> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(path)
        .args([
            "status",
            "--porcelain=v1",
            "--branch",
            "--untracked-files=normal",
        ])
        .env("GIT_OPTIONAL_LOCKS", "0");
    run_command(command, Duration::from_secs(2), stop).map(|output| parse(&output))
}

struct ChildGuard(std::process::Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run_command(mut command: Command, timeout: Duration, stop: &AtomicBool) -> Option<Vec<u8>> {
    if stop.load(Ordering::Relaxed) {
        return None;
    }
    // A file avoids pipe-buffer deadlocks without spawning reader threads.
    // Git writes the branch header first, followed by any dirty entries.
    let mut output = tempfile::tempfile().ok()?;
    let mut child = ChildGuard(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::from(output.try_clone().ok()?))
            .stderr(Stdio::null())
            .spawn()
            .ok()?,
    );
    let began = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) || began.elapsed() >= timeout {
            return None;
        }
        if let Some(status) = child.0.try_wait().ok()? {
            if !status.success() {
                return None;
            }
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    output.seek(SeekFrom::Start(0)).ok()?;
    let mut bytes = Vec::new();
    output.take(65536).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct GitStatus {
    pub dirty: bool,
    pub ahead: u64,
    pub behind: u64,
}

impl fmt::Display for GitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut separator = "";
        if self.dirty {
            write!(f, "●")?;
            separator = " ";
        }
        if self.ahead > 0 {
            write!(f, "{separator}↑{}", self.ahead)?;
            separator = " ";
        }
        if self.behind > 0 {
            write!(f, "{separator}↓{}", self.behind)?;
        }
        Ok(())
    }
}

fn parse(output: &[u8]) -> GitStatus {
    let text = String::from_utf8_lossy(output);
    let mut status = GitStatus::default();
    for line in text.lines() {
        if let Some(branch) = line.strip_prefix("## ") {
            if let Some((_, metadata)) = branch.rsplit_once(" [") {
                for item in metadata.trim_end_matches(']').split(", ") {
                    if let Some(ahead) = item.strip_prefix("ahead ") {
                        status.ahead = ahead.parse().unwrap_or(0);
                    } else if let Some(behind) = item.strip_prefix("behind ") {
                        status.behind = behind.parse().unwrap_or(0);
                    }
                }
            }
        } else if !line.is_empty() {
            status.dirty = true;
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_reports_untracked_and_tracked_changes_and_non_repositories() {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "--quiet"]);
        let worker = start();
        let get_status = |path: &std::path::Path| {
            worker.requests.send(path.to_path_buf()).unwrap();
            let (returned_path, status) =
                worker.results.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(returned_path, path);
            status
        };
        assert_eq!(get_status(repo.path()), Some(GitStatus::default()));
        std::fs::write(repo.path().join("tracked"), "initial").unwrap();
        assert!(get_status(repo.path()).unwrap().dirty);
        git(repo.path(), &["add", "."]);
        git(
            repo.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "initial",
            ],
        );
        assert_eq!(get_status(repo.path()), Some(GitStatus::default()));
        std::fs::write(repo.path().join("tracked"), "changed").unwrap();
        assert!(get_status(repo.path()).unwrap().dirty);
        let outside = tempfile::tempdir().unwrap();
        assert_eq!(get_status(outside.path()), None);
    }

    fn git(path: &std::path::Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(path)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }

    #[cfg(unix)]
    #[test]
    fn command_timeout_returns_promptly() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 5"]);
        let began = Instant::now();
        assert!(run_command(command, Duration::from_millis(60), &AtomicBool::new(false)).is_none());
        assert!(began.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn dropping_worker_cancels_and_reaps_active_child() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("pid");
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let command_pid_file = pid_file.clone();
        let thread = thread::spawn(move || {
            let mut command = Command::new("sh");
            command
                .args(["-c", "echo $$ > \"$1\"; exec sleep 5", "test"])
                .arg(command_pid_file);
            let _ = run_command(command, Duration::from_secs(2), &worker_stop);
        });
        let (requests, _) = mpsc::sync_channel(16);
        let (_, results) = mpsc::channel();
        let worker = GitWorker {
            requests,
            results,
            stop,
            thread: Some(thread),
        };
        let began = Instant::now();
        let pid = loop {
            if let Ok(contents) = std::fs::read_to_string(&pid_file) {
                if let Ok(pid) = contents.trim().parse::<u32>() {
                    break pid;
                }
            }
            assert!(
                began.elapsed() < Duration::from_secs(1),
                "child did not start"
            );
            thread::sleep(Duration::from_millis(10));
        };
        let began = Instant::now();
        drop(worker);
        assert!(began.elapsed() < Duration::from_millis(500));
        assert!(
            !Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
    }

    #[cfg(unix)]
    #[test]
    fn large_command_output_does_not_block_and_is_bounded() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "printf '## main\n'; dd if=/dev/zero bs=1024 count=512 2>/dev/null",
        ]);
        let output = run_command(command, Duration::from_secs(2), &AtomicBool::new(false)).unwrap();
        assert_eq!(output.len(), 65536);
        assert!(parse(&output).dirty);
    }

    #[test]
    fn displays_only_present_markers() {
        assert_eq!(GitStatus::default().to_string(), "");
        assert_eq!(
            GitStatus {
                dirty: true,
                ahead: 3,
                behind: 2
            }
            .to_string(),
            "● ↑3 ↓2"
        );
        assert_eq!(
            GitStatus {
                dirty: false,
                ahead: 0,
                behind: 4
            }
            .to_string(),
            "↓4"
        );
    }

    #[test]
    fn parses_divergence_without_mistaking_branch_name_for_metadata() {
        assert_eq!(
            parse(b"## main...origin/main [ahead 3, behind 2]\n"),
            GitStatus {
                dirty: false,
                ahead: 3,
                behind: 2
            }
        );
        assert_eq!(parse(b"## feature[ahead-42]\n"), GitStatus::default());
        assert_eq!(
            parse(b"## HEAD (no branch)\n M tracked\n"),
            GitStatus {
                dirty: true,
                ..GitStatus::default()
            }
        );
        assert_eq!(
            parse(b"## No commits yet on main\n?? untracked\n"),
            GitStatus {
                dirty: true,
                ..GitStatus::default()
            }
        );
    }
}
