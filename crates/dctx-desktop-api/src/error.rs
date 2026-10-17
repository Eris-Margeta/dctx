//! Desktop error enum and the only text mapping the Tauri layer needs.
//! Implements ARCHITECTURE-PROPOSAL §7 row DX-E4a. Snapshot, settings and recent projects share it.
//! Depends on nothing outside this crate. Callers convert with [`DesktopError::command_string`].
//! One match owns every command string. Variants do not format themselves a second way.

use std::fmt;

/// Failure from snapshot, settings or recent-project storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopError {
    /// The snapshot root is not on disk.
    PathMissing,
    /// A recent-project path is not on disk.
    RecentPathMissing {
        /// Path the caller asked to record.
        path: String,
    },
    /// A project rule file exists but could not be read.
    IgnoreRules {
        /// Filesystem or encoding detail.
        detail: String,
    },
    /// The facade could not open or walk the root.
    Walk {
        /// Facade or filesystem detail.
        detail: String,
    },
    /// The file count is above the desktop limit.
    DirectoryTooLarge {
        /// Files counted before contents are read.
        files: u64,
        /// `file_count_limit` from desktop settings.
        limit: u64,
    },
    /// Active filters kept no files.
    AllFiltered,
    /// The facade page was not valid text.
    Format {
        /// Encoding detail.
        detail: String,
    },
    /// The injected recent-project directory could not be created.
    CreateStorageDir {
        /// Filesystem detail.
        detail: String,
    },
    /// The injected settings directory could not be created.
    CreateConfigDir {
        /// Filesystem detail.
        detail: String,
    },
    /// `ensure_config_dirs` could not create the config directory.
    CreateDesktopConfigDir {
        /// Filesystem detail.
        detail: String,
    },
    /// `ensure_config_dirs` could not create the cache directory.
    CreateDesktopCacheDir {
        /// Filesystem detail.
        detail: String,
    },
    /// Settings could not be written.
    WriteConfig {
        /// Filesystem detail.
        detail: String,
    },
    /// The recent-project file could not be read.
    ReadRecent {
        /// Filesystem detail.
        detail: String,
    },
    /// The recent-project file was not valid JSON.
    ParseRecent {
        /// Parser detail.
        detail: String,
    },
    /// The recent-project list could not be encoded.
    SerializeRecent {
        /// Encoder detail.
        detail: String,
    },
    /// The recent-project file could not be written.
    WriteRecent {
        /// Filesystem detail.
        detail: String,
    },
    /// `remove` was asked for a path that is not stored.
    RecentMissing {
        /// Path the caller asked to drop.
        path: String,
    },
}

impl DesktopError {
    /// The only string the Tauri command layer returns for this error.
    pub fn command_string(&self) -> String {
        match self {
            Self::PathMissing => "Path does not exist".to_string(),
            Self::RecentPathMissing { path } => format!("Path does not exist: {path}"),
            Self::IgnoreRules { detail } => format!("Error loading ignore rules: {detail}"),
            Self::Walk { detail } => format!("Failed to walk directory: {detail}"),
            Self::DirectoryTooLarge { files, limit } => format!(
                "Directory too large: {files} files (limit: {limit})\n\nPlease use filters or select a smaller directory."
            ),
            Self::AllFiltered => "All files filtered out".to_string(),
            Self::Format { detail } => format!("Failed to format output: {detail}"),
            Self::CreateStorageDir { detail } => format!("Failed to create config directory: {detail}"),
            Self::CreateConfigDir { detail } => format!("Failed to create config dir: {detail}"),
            Self::CreateDesktopConfigDir { detail } => {
                format!("Failed to create desktop config dir: {detail}")
            }
            Self::CreateDesktopCacheDir { detail } => {
                format!("Failed to create desktop cache dir: {detail}")
            }
            Self::WriteConfig { detail } => format!("Failed to write config: {detail}"),
            Self::ReadRecent { detail } => format!("Failed to read recent projects file: {detail}"),
            Self::ParseRecent { detail } => format!("Failed to parse recent projects: {detail}"),
            Self::SerializeRecent { detail } => format!("Failed to serialize recent projects: {detail}"),
            Self::WriteRecent { detail } => format!("Failed to write recent projects file: {detail}"),
            Self::RecentMissing { path } => format!("Project not found in recent list: {path}"),
        }
    }
}

impl fmt::Display for DesktopError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.command_string())
    }
}

impl std::error::Error for DesktopError {}

impl From<DesktopError> for String {
    fn from(error: DesktopError) -> Self {
        error.command_string()
    }
}

#[cfg(test)]
mod tests {
    use super::DesktopError;

    #[test]
    fn command_strings_match_the_desktop_commands() {
        let large = DesktopError::DirectoryTooLarge { files: 3, limit: 2 };
        assert_eq!(
            large.command_string(),
            "Directory too large: 3 files (limit: 2)\n\nPlease use filters or select a smaller directory."
        );
        assert_eq!(String::from(large), DesktopError::DirectoryTooLarge { files: 3, limit: 2 }.to_string());
        assert_eq!(DesktopError::PathMissing.command_string(), "Path does not exist");
        assert_eq!(
            DesktopError::RecentPathMissing { path: "/missing".to_string() }.command_string(),
            "Path does not exist: /missing"
        );
        assert_eq!(DesktopError::AllFiltered.command_string(), "All files filtered out");
        assert_eq!(
            DesktopError::IgnoreRules { detail: "denied".to_string() }.command_string(),
            "Error loading ignore rules: denied"
        );
        assert_eq!(
            DesktopError::Walk { detail: "root".to_string() }.command_string(),
            "Failed to walk directory: root"
        );
        assert_eq!(
            DesktopError::Format { detail: "utf-8".to_string() }.command_string(),
            "Failed to format output: utf-8"
        );
        assert_eq!(
            DesktopError::RecentMissing { path: "/p".to_string() }.command_string(),
            "Project not found in recent list: /p"
        );
    }
}
