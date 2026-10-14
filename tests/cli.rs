use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn run(data: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zort"))
        .env("_ZO_DATA_DIR", data)
        .args(args)
        .output()
        .unwrap()
}
fn list(data: &Path, args: &[&str]) -> Value {
    let output = run(data, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn database(data: &Path, text: String) {
    let entries: Vec<(String, f64, u64)> = text
        .lines()
        .map(|line| {
            let mut parts = line.splitn(3, '\t');
            let time = parts.next().unwrap().parse().unwrap();
            let rank = parts.next().unwrap().parse().unwrap();
            let path = parts.next().unwrap().to_owned();
            (path, rank, time)
        })
        .collect();
    let mut bytes = 3_u32.to_le_bytes().to_vec();
    bytes.extend(bincode::serialize(&entries).unwrap());
    fs::write(data.join("db.zo"), bytes).unwrap();
}
fn git(path: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    );
}
fn fixture() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().canonicalize().unwrap().join("data");
    fs::create_dir(&data).unwrap();
    let repo = t.path().canonicalize().unwrap().join("my project");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "feature/picker"]);
    fs::create_dir(repo.join("src")).unwrap();
    (t, data, repo)
}
#[test]
fn preserves_visited_directories_with_their_own_scores_and_times() {
    let (_t, data, repo) = fixture();
    database(
        &data,
        format!(
            "1700000000\t2\t{}\n1700000060\t3\t{}\n",
            repo.display(),
            repo.join("src").display()
        ),
    );
    let rows = list(&data, &["list", "--json"]);
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert_eq!(rows[0]["path"], repo.join("src").to_str().unwrap());
    assert_eq!(rows[0]["name"], "src");
    assert_eq!(rows[0]["branch"], "feature/picker");
    assert_eq!(rows[0]["last_accessed"], 1700000060_u64);
    assert_eq!(rows[0]["score"], 0.8);
    assert_eq!(rows[1]["path"], repo.to_str().unwrap());
    assert_eq!(rows[1]["last_accessed"], 1700000000_u64);
    assert_eq!(rows[1]["score"], 0.5);
    let chosen = run(&data, &["pick", "--query", "src"]);
    assert!(chosen.status.success());
    assert_eq!(
        String::from_utf8(chosen.stdout).unwrap().trim_end(),
        repo.join("src").to_str().unwrap()
    );
}
#[test]
fn sorts_by_frecency_and_filters_fuzzy_queries() {
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("alpha");
    let b = t.path().join("beta");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    database(
        t.path(),
        format!(
            "1700000000\t2\t{}\n1700000000\t10\t{}\n",
            a.display(),
            b.display()
        ),
    );
    let rows = list(t.path(), &["list", "--json"]);
    assert_eq!(rows[0]["name"], "beta");
    let rows = list(t.path(), &["list", "--json", "apa"]);
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["name"], "alpha");
    assert_eq!(rows[0]["branch"], Value::Null);
}
#[test]
fn keeps_worktrees_separate_and_handles_detached_head() {
    let (t, data, repo) = fixture();
    git(
        &repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    );
    let worktree = t.path().canonicalize().unwrap().join("worktree");
    git(
        &repo,
        &["worktree", "add", "--detach", worktree.to_str().unwrap()],
    );
    database(
        &data,
        format!(
            "1700000000\t2\t{}\n1700000000\t3\t{}\n",
            repo.display(),
            worktree.display()
        ),
    );
    let rows = list(&data, &["list", "--json"]);
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert!(rows[0]["branch"].as_str().unwrap().starts_with("detached:"));
}
#[test]
fn reads_legacy_timestamps() {
    let (_t, data, repo) = fixture();
    let mut bytes = 3_u32.to_le_bytes().to_vec();
    bytes.extend(
        bincode::serialize(&vec![(repo.to_str().unwrap(), 8_f64, 1700000000_u64)]).unwrap(),
    );
    fs::write(data.join("db.zo"), bytes).unwrap();
    let rows = list(&data, &["list", "--json"]);
    assert_eq!(rows[0]["last_accessed"], 1700000000_u64);
}
#[test]
fn zsh_integration_changes_directory_and_binds_shortcut() {
    let (t, data, repo) = fixture();
    database(&data, format!("1700000000\t2\t{}\n", repo.display()));
    let init = run(&data, &["init", "zsh", "--cmd", "j", "--bind", "ctrl-g"]);
    assert!(init.status.success());
    let script = t.path().join("init.zsh");
    fs::write(&script, init.stdout).unwrap();
    let output = Command::new("zsh")
        .args([
            "-f",
            "-c",
            r#"source "$1"; j --query 'my project'; print -r -- "$PWD"; bindkey '^G'"#,
            "test",
        ])
        .arg(script)
        .env("_ZO_DATA_DIR", &data)
        .env(
            "PATH",
            format!(
                "{}:{}",
                Path::new(env!("CARGO_BIN_EXE_zort"))
                    .parent()
                    .unwrap()
                    .display(),
                std::env::var("PATH").unwrap()
            ),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(repo.to_str().unwrap()));
    assert!(String::from_utf8_lossy(&output.stdout).contains("_zort_widget"));
}
#[test]
fn rejects_shell_injection_in_configurable_names() {
    let t = tempfile::tempdir().unwrap();
    assert!(
        !run(
            t.path(),
            &["init", "zsh", "--cmd", "j;touch /tmp/zort-test"]
        )
        .status
        .success()
    );
    assert!(
        !run(
            t.path(),
            &["init", "zsh", "--bind", "ctrl-g'$(touch /tmp/zort-test)"]
        )
        .status
        .success()
    );
}

#[test]
fn rejects_names_that_break_generated_shell_commands() {
    let t = tempfile::tempdir().unwrap();
    for name in [
        "command", "local", "builtin", "zle", "bindkey", "return", "if",
    ] {
        let output = run(t.path(), &["init", "zsh", "--cmd", name, "--bind", "none"]);
        assert!(
            !output.status.success(),
            "accepted conflicting shell command: {name}"
        );
    }
}

#[test]
fn reads_text_database_access_times() {
    let (_t, data, repo) = fixture();
    database(&data, format!("1700000000\t2\t{}\n", repo.display()));
    fs::write(
        data.join("db.txt"),
        format!("1700000600\t2.00\t{}\n", repo.display()),
    )
    .unwrap();
    let rows = list(&data, &["list", "--json"]);
    assert_eq!(rows[0]["last_accessed"], 1700000600_u64);
}
