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
            Err(error) => Err(error.into()),
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
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().context("path has no parent")?)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}
