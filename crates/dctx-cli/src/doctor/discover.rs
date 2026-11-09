// SPDX-License-Identifier: Apache-2.0
//! Discover visible executable paths without changing directories or following cache links.

use std::{
    collections::BTreeSet,
    env, fs,
    path::{Component, Path, PathBuf},
};

use super::Scope;

pub(super) struct Context {
    pub cwd: PathBuf,
    pub running: PathBuf,
    pub path: Vec<PathBuf>,
    pub virtual_env: Option<PathBuf>,
    pub explicit: Option<PathBuf>,
}

impl Context {
    pub fn from_env() -> Result<Self, String> {
        let cwd = env::current_dir().map_err(|error| format!("current directory: {error}"))?;
        let at_cwd = |key| {
            env::var_os(key)
                .filter(|value| !value.is_empty())
                .map(|value| absolute(&cwd, Path::new(&value)))
        };
        Ok(Self {
            running: env::current_exe().map_err(|error| format!("running executable: {error}"))?,
            path: env::var_os("PATH")
                .map(|value| env::split_paths(&value).collect())
                .unwrap_or_default(),
            virtual_env: at_cwd("VIRTUAL_ENV"),
            explicit: at_cwd("DCTX_BIN"),
            cwd,
        })
    }

    pub fn candidates(&self) -> Vec<PathBuf> {
        let mut paths = BTreeSet::new();
        let directories = self
            .path
            .iter()
            .map(|path| absolute(&self.cwd, path))
            .chain(std::iter::once(self.cwd.join("node_modules/.bin")))
            .chain(
                self.virtual_env
                    .iter()
                    .map(|root| root.join(if cfg!(windows) { "Scripts" } else { "bin" })),
            );
        for directory in directories {
            for name in executable_names() {
                let path = directory.join(name);
                if executable(&path) {
                    paths.insert(path);
                }
            }
        }
        // A broken explicit override is useful diagnostic information too.
        paths.extend(self.explicit.iter().cloned());
        paths.insert(self.invocation_path());
        paths.insert(absolute(&self.cwd, &self.running));
        paths.into_iter().collect()
    }

    /// True when doctor may run `path --version` without `--probe-project`: the running file, `DCTX_BIN`,
    /// or a file found through an absolute PATH entry. Candidates reached only through `./node_modules/.bin`,
    /// `VIRTUAL_ENV` or a relative PATH entry can be planted by an untrusted checkout and are never executed
    /// by default.
    pub fn trusted_to_execute(&self, path: &Path) -> bool {
        if self.explicit.as_deref() == Some(path)
            || path == self.invocation_path()
            || path == absolute(&self.cwd, &self.running)
        {
            return true;
        }
        path.parent().is_some_and(|parent| {
            self.path
                .iter()
                .any(|entry| entry.is_absolute() && absolute(&self.cwd, entry) == parent)
        })
    }

    pub fn scope(&self, path: &Path) -> Scope {
        if self.explicit.as_deref() == Some(path) {
            return Scope::Explicit;
        }
        let real = fs::canonicalize(path).ok();
        let project_root = self.cwd.ancestors().find(|root| {
            ["Cargo.toml", "package.json", "pyproject.toml"]
                .iter()
                .any(|name| root.join(name).is_file())
        });
        let project_cargo = project_root.is_some_and(|root| path.starts_with(root))
            && path
                .ancestors()
                .skip(1)
                .any(|parent| parent.join(".crates.toml").is_file());
        if project_cargo
            || project_path(path, self.virtual_env.as_deref())
            || real
                .as_deref()
                .is_some_and(|real| project_path(real, self.virtual_env.as_deref()))
        {
            Scope::Project
        } else {
            Scope::System
        }
    }

    /// Preserve the ownership of an invoked symlink even when current_exe resolves its target.
    pub fn invocation_path(&self) -> PathBuf {
        let Some(argument) = env::args_os().next() else {
            return self.running.clone();
        };
        let argument = PathBuf::from(argument);
        let local = absolute(&self.cwd, &argument);
        if same_file(&local, &self.running) {
            return local;
        }
        if argument.components().count() == 1 {
            for directory in &self.path {
                let candidate = absolute(&self.cwd, directory).join(&argument);
                if same_file(&candidate, &self.running) {
                    return candidate;
                }
            }
        }
        self.running.clone()
    }
}

fn project_path(path: &Path, virtual_env: Option<&Path>) -> bool {
    path.components()
        .any(|part| part.as_os_str() == "node_modules")
        || virtual_env.is_some_and(|root| {
            path.starts_with(root)
                || fs::canonicalize(root).is_ok_and(|root| path.starts_with(root))
        })
        || path
            .ancestors()
            .skip(1)
            .any(|parent| parent.join("pyvenv.cfg").is_file())
}

fn executable_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &[
            "dctx.exe",
            "dctx.com",
            "dctx.cmd",
            "dctx.bat",
            "dctx",
            "dctx-cli.exe",
            "dctx-cli.com",
            "dctx-cli.cmd",
            "dctx-cli.bat",
            "dctx-cli",
        ]
    } else {
        &["dctx", "dctx-cli"]
    }
}

fn executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Normalizes relative PATH entries lexically, preserving the visible symlink path.
fn absolute(cwd: &Path, path: &Path) -> PathBuf {
    let joined = cwd.join(path);
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

pub(super) fn same_file(left: &Path, right: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(left), Ok(right)) = (fs::metadata(left), fs::metadata(right)) {
            return (left.dev(), left.ino()) == (right.dev(), right.ino());
        }
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}
