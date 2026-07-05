use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::paths::ContextPaths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderLock {
    pub ocl_version: String,
    pub objects_digest: String,
    pub targets: HashMap<String, TargetLock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetLock {
    pub path: String,
    pub content_digest: String,
    pub object_ids: Vec<String>,
}

impl RenderLock {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&raw)?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let raw = serde_yaml::to_string(self)?;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?;
        file.write_all(raw.as_bytes())?;
        file.sync_data()?;
        Ok(())
    }
}

pub fn digest_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

pub struct OperationLock {
    _inner: FileLock,
}

impl OperationLock {
    pub fn acquire(paths: &ContextPaths) -> anyhow::Result<Self> {
        fs::create_dir_all(&paths.cache)?;
        let path = paths.cache.join("operation.lock");
        Ok(Self {
            _inner: FileLock::acquire(&path, "ctx write operation")?,
        })
    }
}

pub struct FileLock {
    path: PathBuf,
}

impl FileLock {
    pub fn acquire(path: &Path, label: &str) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if stale_lock(path) {
            let _ = fs::remove_file(path);
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    anyhow::anyhow!("another {label} is in progress (lock: {})", path.display())
                } else {
                    anyhow::Error::new(err)
                }
            })?;
        writeln!(file, "pid={}", std::process::id())?;
        writeln!(
            file,
            "created={}",
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        )?;
        file.sync_data()?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

fn stale_lock(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed > Duration::from_secs(60 * 60))
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
