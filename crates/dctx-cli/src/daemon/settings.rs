// SPDX-License-Identifier: Apache-2.0
use super::*;

pub(super) struct Settings {
    pub enabled: bool,
    pub idle: Duration,
    pub endpoint: PathBuf,
}

pub(super) fn enabled() -> bool {
    match std::env::var("DCTX_DAEMON").ok().as_deref() {
        Some("1" | "on" | "true") => true,
        Some(_) => false,
        None => dctx_cli::host::cache_root_dir()
            .ok()
            .and_then(|root| read_config(&root.join("daemon")).ok())
            .and_then(|value| value.get("enabled").and_then(Value::as_bool))
            .unwrap_or(false),
    }
}

fn root() -> io::Result<PathBuf> {
    let root = dctx_cli::host::cache_root_dir().map_err(io::Error::other)?;
    dctx_cli::host::ensure_cache_dir(&root).map_err(io::Error::other)?;
    let directory = root.join("daemon");
    transport::private_directory(&directory)?;
    Ok(directory)
}

fn read_config(directory: &Path) -> io::Result<Value> {
    let path = directory.join("config.json");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Value::Null),
        Err(error) => return Err(error),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsafe or oversized daemon config",
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(4097)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}

impl Settings {
    pub fn load() -> io::Result<Self> {
        let directory = root()?;
        let config = read_config(&directory)?;
        let enabled = match std::env::var("DCTX_DAEMON").ok().as_deref() {
            Some("1" | "on" | "true") => true,
            Some("0" | "off" | "false") => false,
            None => config
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "DCTX_DAEMON must be 1 or 0",
                ))
            }
        };
        let idle = std::env::var("DCTX_DAEMON_IDLE_SECONDS")
            .ok()
            .map(|value| value.parse::<u64>())
            .transpose()
            .map_err(io::Error::other)?
            .unwrap_or(300);
        if !(1..=86400).contains(&idle) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "DCTX_DAEMON_IDLE_SECONDS must be 1..86400",
            ));
        }
        let mut hash = dctx_hash::Sha256::new();
        hash.update(directory.as_os_str().as_encoded_bytes())
            .map_err(io::Error::other)?;
        let endpoint = transport::endpoint(&directory, &hash.finalize().to_string());
        Ok(Self {
            enabled,
            idle: Duration::from_secs(idle),
            endpoint,
        })
    }
}

pub(super) fn store(enabled: bool) -> io::Result<()> {
    let directory = root()?;
    let path = directory.join("config.json");
    if std::fs::symlink_metadata(&path)
        .is_ok_and(|meta| !meta.is_file() || meta.file_type().is_symlink())
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unsafe daemon config",
        ));
    }
    let temporary = directory.join(format!("config-{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let result = (|| {
        file.write_all(serde_json::to_string(&json!({"enabled":enabled}))?.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
