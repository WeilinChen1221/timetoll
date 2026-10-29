use crate::{config::Config, engine::Ledger};
use anyhow::{Context, Result, ensure};
use directories::ProjectDirs;
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Store {
    pub dir: PathBuf,
}

impl Store {
    pub fn new(dir: Option<PathBuf>) -> Result<Self> {
        let dir = match dir {
            Some(path) => path,
            None => ProjectDirs::from("dev", "timetoll", "timetoll")
                .context("cannot find user config directory; use --data-dir")?
                .config_dir()
                .to_path_buf(),
        };
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn lock(&self, name: &str) -> Result<File> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.dir.join(name))?;
        file.try_lock_exclusive().with_context(|| {
            format!(
                "{} is busy; another timetoll process may be running",
                self.dir.join(name).display()
            )
        })?;
        Ok(file)
    }

    pub fn init(&self) -> Result<()> {
        let _lock = self.lock("config.lock")?;
        ensure!(
            !self.dir.join("config.toml").exists(),
            "config already exists at {}",
            self.dir.display()
        );
        self.save_config(&Config::default())
    }

    pub fn config(&self) -> Result<Config> {
        let path = self.dir.join("config.toml");
        let config: Config = toml::from_str(&fs::read_to_string(&path).with_context(|| {
            format!("cannot read {}; run timetoll init first", path.display())
        })?)?;
        config.validate()?;
        Ok(config)
    }

    pub fn save_config(&self, config: &Config) -> Result<()> {
        config.validate()?;
        atomic_write(
            &self.dir.join("config.toml"),
            toml::to_string_pretty(config)?.as_bytes(),
        )
    }

    pub fn ledger(&self) -> Result<Ledger> {
        let path = self.dir.join("state.json");
        match fs::read(&path) {
            Ok(data) => {
                let ledger: Ledger = serde_json::from_slice(&data)
                    .context("state is corrupt; restore state.json from a backup")?;
                ledger.validate()?;
                Ok(ledger)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Ledger::default()),
            Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    pub fn save_ledger(&self, ledger: &Ledger) -> Result<()> {
        atomic_write(
            &self.dir.join("state.json"),
            &serde_json::to_vec_pretty(ledger)?,
        )
    }
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("path has no parent")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("cannot create temporary file for {}", path.display()))?;
    temp.write_all(bytes)
        .with_context(|| format!("cannot write temporary file for {}", path.display()))?;
    temp.as_file()
        .sync_all()
        .with_context(|| format!("cannot sync temporary file for {}", path.display()))?;
    #[cfg(windows)]
    let result = persist_with_retry(temp, path, |temp, path| temp.persist(path));
    #[cfg(not(windows))]
    let result = temp.persist(path).map(|_| ()).map_err(|e| e.error);
    result.with_context(|| format!("cannot atomically replace {}", path.display()))?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|dir| dir.sync_all())
        .with_context(|| format!("cannot sync directory {}", parent.display()))?;
    Ok(())
}

#[cfg(any(windows, test))]
fn persist_with_retry(
    mut temp: tempfile::NamedTempFile,
    path: &Path,
    mut persist: impl FnMut(
        tempfile::NamedTempFile,
        &Path,
    ) -> std::result::Result<File, tempfile::PersistError>,
) -> std::io::Result<()> {
    // Windows replacement can briefly fail while a reader or scanner holds a
    // file open. Keep the same complete, synced file for each attempt. Never
    // delete the destination or fall back to truncating it in place.
    const RETRIES: usize = 20;
    for attempt in 0..=RETRIES {
        match persist(temp, path) {
            Ok(_) => return Ok(()),
            Err(error) => {
                // ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION,
                // ERROR_LOCK_VIOLATION. Permanent denials still fail after 500ms.
                if attempt == RETRIES || !matches!(error.error.raw_os_error(), Some(5 | 32 | 33)) {
                    return Err(error.error);
                }
                temp = error.file;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
    }
    unreachable!("the final attempt returns success or an error")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_survives_transient_windows_access_denied() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let old = Ledger::default();
        let updated = Ledger {
            balance_ms: 12345,
            ..old.clone()
        };
        fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
        let mut temp = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        temp.write_all(&serde_json::to_vec(&updated).unwrap())
            .unwrap();
        temp.as_file().sync_all().unwrap();
        let mut attempts = 0;
        let result = persist_with_retry(temp, &path, |temp, path| {
            attempts += 1;
            if attempts <= 3 {
                // Replay the captured Windows error at the replacement boundary.
                assert_eq!(
                    serde_json::from_slice::<Ledger>(&fs::read(path).unwrap()).unwrap(),
                    old
                );
                Err(tempfile::PersistError {
                    error: std::io::Error::from_raw_os_error([5, 32, 33][attempts - 1]),
                    file: temp,
                })
            } else {
                temp.persist(path)
            }
        });
        assert!(result.is_ok(), "checkpoint aborted: {result:?}");
        assert_eq!(attempts, 4);
        assert_eq!(
            serde_json::from_slice::<Ledger>(&fs::read(path).unwrap()).unwrap(),
            updated
        );
    }

    #[test]
    fn failed_replacement_preserves_previous_state_and_cleans_up_temp_file() {
        // Persistent access/sharing errors are bounded; unrelated errors fail
        // immediately. Neither case may remove or truncate the previous state.
        for (code, expected_attempts) in [(5, 21), (32, 21), (33, 21), (112, 1)] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("state.json");
            fs::write(&path, b"previous state").unwrap();
            let mut temp = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
            temp.write_all(b"replacement state").unwrap();
            let temp_path = temp.path().to_owned();
            let mut attempts = 0;
            let error = persist_with_retry(temp, &path, |temp, _| {
                attempts += 1;
                Err(tempfile::PersistError {
                    error: std::io::Error::from_raw_os_error(code),
                    file: temp,
                })
            })
            .unwrap_err();
            assert_eq!(error.raw_os_error(), Some(code));
            assert_eq!(attempts, expected_attempts);
            assert_eq!(fs::read(&path).unwrap(), b"previous state");
            assert!(!temp_path.exists());
        }
    }

    #[test]
    fn replacement_error_identifies_operation_and_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::create_dir(&path).unwrap();
        let error = atomic_write(&path, b"state").unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("cannot atomically replace"), "{message}");
        assert!(message.contains(&path.display().to_string()), "{message}");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn replacement_recovers_after_real_windows_sharing_conflict() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, b"previous state").unwrap();
        // Deliberately omit FILE_SHARE_DELETE to force a real OS denial.
        let mut reader = Some(
            OpenOptions::new()
                .read(true)
                .share_mode(3)
                .open(&path)
                .unwrap(),
        );
        let mut temp = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        temp.write_all(b"replacement state").unwrap();
        temp.as_file().sync_all().unwrap();
        let mut attempts = 0;
        persist_with_retry(temp, &path, |temp, path| {
            attempts += 1;
            let result = temp.persist(path);
            if attempts == 1 {
                let error = result.as_ref().unwrap_err();
                assert!(matches!(error.error.raw_os_error(), Some(5 | 32 | 33)));
                assert_eq!(fs::read(path).unwrap(), b"previous state");
                drop(reader.take());
            }
            result
        })
        .unwrap();
        assert!(attempts >= 2);
        assert_eq!(fs::read(&path).unwrap(), b"replacement state");
    }
}
