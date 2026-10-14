use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Preferences {
    pub pins: Vec<PathBuf>,
    pub hidden: Vec<PathBuf>,
}

enum Change {
    Pin(bool),
    Hidden(bool),
}

impl Preferences {
    pub fn load() -> Result<Self> {
        Self::load_at(&config_directory()?)
    }

    pub fn set_pin(path: &Path, enabled: bool) -> Result<Self> {
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        Self::update_at(&config_directory()?, &path, Change::Pin(enabled))
    }

    pub fn set_hidden(path: &Path, enabled: bool) -> Result<Self> {
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        Self::update_at(&config_directory()?, &path, Change::Hidden(enabled))
    }

    fn load_at(directory: &Path) -> Result<Self> {
        let path = directory.join("preferences.json");
        match File::open(&path) {
            Ok(file) => serde_json::from_reader(file)
                .with_context(|| format!("Cannot read preferences from {}", path.display())),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => {
                Err(error).with_context(|| format!("Cannot open preferences at {}", path.display()))
            }
        }
    }

    fn update_at(directory: &Path, path: &Path, change: Change) -> Result<Self> {
        fs::create_dir_all(directory).with_context(|| {
            format!(
                "Cannot create configuration directory {}",
                directory.display()
            )
        })?;
        // Keep the lock on a separate file: replacing preferences.json must not replace the lock.
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("preferences.lock"))
            .context("Cannot open preferences lock")?;
        lock.lock_exclusive().context("Cannot lock preferences")?;
        let mut preferences = Self::load_at(directory)?;
        match change {
            Change::Pin(enabled) => {
                set_membership(&mut preferences.pins, path, enabled);
                if enabled {
                    set_membership(&mut preferences.hidden, path, false);
                }
            }
            Change::Hidden(enabled) => {
                set_membership(&mut preferences.hidden, path, enabled);
                if enabled {
                    set_membership(&mut preferences.pins, path, false);
                }
            }
        }
        let mut temporary =
            NamedTempFile::new_in(directory).context("Cannot create temporary preferences file")?;
        serde_json::to_writer_pretty(&mut temporary, &preferences)
            .context("Cannot serialize preferences")?;
        temporary.write_all(b"\n")?;
        temporary
            .as_file()
            .sync_all()
            .context("Cannot sync preferences")?;
        temporary
            .persist(directory.join("preferences.json"))
            .context("Cannot save preferences")?;
        Ok(preferences)
    }

    pub fn is_pinned(&self, path: &Path) -> bool {
        self.pins.iter().any(|pin| pin == path)
    }

    pub fn is_hidden(&self, path: &Path) -> bool {
        self.hidden.iter().any(|hidden| hidden == path)
    }
}

fn config_directory() -> Result<PathBuf> {
    if let Some(directory) = std::env::var_os("ZURF_CONFIG_DIR") {
        return Ok(PathBuf::from(directory));
    }
    dirs::config_dir()
        .map(|directory| directory.join("zurf"))
        .context("Cannot determine configuration directory")
}

fn set_membership(paths: &mut Vec<PathBuf>, path: &Path, enabled: bool) {
    paths.retain(|existing| existing != path);
    if enabled {
        paths.push(path.to_path_buf());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs2::FileExt;
    use std::fs::{self, OpenOptions};
    use std::sync::{Arc, Barrier, mpsc};
    use std::time::Duration;

    #[test]
    fn missing_preferences_are_empty_without_creating_files() {
        let directory = tempfile::tempdir().unwrap();
        let preferences = Preferences::load_at(directory.path()).unwrap();
        assert!(preferences.pins.is_empty());
        assert!(preferences.hidden.is_empty());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn pin_and_hidden_changes_persist_and_can_be_removed() {
        let directory = tempfile::tempdir().unwrap();
        let pinned = Path::new("/projects/pinned");
        let hidden = Path::new("/projects/hidden");
        Preferences::update_at(directory.path(), pinned, Change::Pin(true)).unwrap();
        Preferences::update_at(directory.path(), pinned, Change::Pin(true)).unwrap();
        Preferences::update_at(directory.path(), hidden, Change::Hidden(true)).unwrap();
        let loaded = Preferences::load_at(directory.path()).unwrap();
        assert_eq!(loaded.pins, vec![pinned.to_path_buf()]);
        assert_eq!(loaded.hidden, vec![hidden.to_path_buf()]);
        assert!(loaded.is_pinned(pinned));
        assert!(loaded.is_hidden(hidden));
        Preferences::update_at(directory.path(), pinned, Change::Pin(false)).unwrap();
        Preferences::update_at(directory.path(), hidden, Change::Hidden(false)).unwrap();
        let loaded = Preferences::load_at(directory.path()).unwrap();
        assert!(loaded.pins.is_empty());
        assert!(loaded.hidden.is_empty());
    }

    #[test]
    fn pinning_unhides_and_hiding_unpins() {
        let directory = tempfile::tempdir().unwrap();
        let project = Path::new("/projects/project");
        Preferences::update_at(directory.path(), project, Change::Hidden(true)).unwrap();
        let pinned = Preferences::update_at(directory.path(), project, Change::Pin(true)).unwrap();
        assert!(pinned.is_pinned(project));
        assert!(!pinned.is_hidden(project));
        let hidden =
            Preferences::update_at(directory.path(), project, Change::Hidden(true)).unwrap();
        assert!(!hidden.is_pinned(project));
        assert!(hidden.is_hidden(project));
    }

    #[test]
    fn corrupt_preferences_are_reported_without_overwriting_them() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("preferences.json");
        fs::write(&file, b"broken json").unwrap();
        assert!(Preferences::load_at(directory.path()).is_err());
        assert!(
            Preferences::update_at(directory.path(), Path::new("/project"), Change::Pin(true))
                .is_err()
        );
        assert_eq!(fs::read(file).unwrap(), b"broken json");
    }

    #[test]
    fn concurrent_updates_preserve_every_pin() {
        let directory = tempfile::tempdir().unwrap();
        let barrier = Arc::new(Barrier::new(16));
        let handles: Vec<_> = (0..16)
            .map(|index| {
                let directory = directory.path().to_path_buf();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    Preferences::update_at(
                        &directory,
                        &PathBuf::from(format!("/projects/{index}")),
                        Change::Pin(true),
                    )
                    .unwrap();
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let preferences = Preferences::load_at(directory.path()).unwrap();
        assert_eq!(preferences.pins.len(), 16);
        for index in 0..16 {
            assert!(preferences.is_pinned(&PathBuf::from(format!("/projects/{index}"))));
        }
    }

    #[test]
    fn updates_wait_for_lock_before_reading_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.path().join("preferences.lock"))
            .unwrap();
        lock.lock_exclusive().unwrap();
        let worker_directory = directory.path().to_path_buf();
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = Preferences::update_at(
                &worker_directory,
                Path::new("/projects/new"),
                Change::Pin(true),
            );
            sender.send(result).unwrap();
        });
        assert!(matches!(
            receiver.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        fs::write(
            directory.path().join("preferences.json"),
            r#"{"pins":["/projects/existing"],"hidden":[]}"#,
        )
        .unwrap();
        FileExt::unlock(&lock).unwrap();
        let preferences = receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        worker.join().unwrap();
        assert!(preferences.is_pinned(Path::new("/projects/existing")));
        assert!(preferences.is_pinned(Path::new("/projects/new")));
    }
}
