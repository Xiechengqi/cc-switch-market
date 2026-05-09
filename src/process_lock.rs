use std::{fs, io::Write, path::PathBuf};

use anyhow::{Context, bail};
use fs2::FileExt;

use crate::config::config_dir;

pub struct ServerLock {
    _file: fs::File,
    pid_path: PathBuf,
}

impl ServerLock {
    pub fn acquire() -> anyhow::Result<Self> {
        let dir = config_dir()?;
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let lock_path = dir.join("cc-switch-market.lock");
        let pid_path = dir.join("cc-switch-market.pid");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("open {}", lock_path.display()))?;
        if file.try_lock_exclusive().is_err() {
            bail!("cc-switch-market is already running; stop it before running this command");
        }
        file.set_len(0)?;
        writeln!(file, "{}", std::process::id())?;
        fs::write(&pid_path, std::process::id().to_string())
            .with_context(|| format!("write {}", pid_path.display()))?;
        Ok(Self {
            _file: file,
            pid_path,
        })
    }

    pub fn assert_not_running() -> anyhow::Result<()> {
        let dir = config_dir()?;
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let lock_path = dir.join("cc-switch-market.lock");
        let file = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("open {}", lock_path.display()))?;
        if file.try_lock_exclusive().is_err() {
            bail!("cc-switch-market is running. Stop the market process before logout");
        }
        let pid_path = dir.join("cc-switch-market.pid");
        if pid_path.exists() {
            let _ = fs::remove_file(pid_path);
        }
        Ok(())
    }
}

impl Drop for ServerLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.pid_path);
    }
}
