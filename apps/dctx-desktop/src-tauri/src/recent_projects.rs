// apps/dctx-desktop/src-tauri/src/recent_projects.rs
//
// Recent Projects Storage Backend
// Manages persistent storage of recently accessed projects

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Maximum number of recent projects to store
const MAX_RECENT_PROJECTS: usize = 20;

/// RecentProject represents a single project in the recent list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentProject {
    pub path: String,
    pub name: String,
    pub last_accessed: String, // ISO 8601 timestamp
    pub access_count: u32,
}

/// Get path to recent projects storage file
fn get_recent_projects_path() -> Result<PathBuf, String> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| "Failed to get config directory".to_string())?;

    let dctx_config = config_dir.join("dctx-desktop");

    // Ensure directory exists
    if !dctx_config.exists() {
        fs::create_dir_all(&dctx_config)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    Ok(dctx_config.join("recent_projects.json"))
}

/// Load recent projects from disk
pub fn load_recent_projects() -> Result<Vec<RecentProject>, String> {
    let path = get_recent_projects_path()?;

    if !path.exists() {
        // Return empty list if file doesn't exist yet
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read recent projects file: {}", e))?;

    let projects: Vec<RecentProject> = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse recent projects: {}", e))?;

    Ok(projects)
}

/// Save recent projects to disk
fn save_recent_projects(projects: &[RecentProject]) -> Result<(), String> {
    let path = get_recent_projects_path()?;

    let json = serde_json::to_string_pretty(projects)
        .map_err(|e| format!("Failed to serialize recent projects: {}", e))?;

    fs::write(&path, json)
        .map_err(|e| format!("Failed to write recent projects file: {}", e))?;

    Ok(())
}

/// Add or update a project in recent projects list
pub fn add_recent_project(path: &str) -> Result<(), String> {
    // Validate path exists
    let project_path = Path::new(path);
    if !project_path.exists() {
        return Err(format!("Path does not exist: {}", path));
    }

    // Extract project name from path
    let name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown")
        .to_string();

    // Load existing projects
    let mut projects = load_recent_projects()?;

    // Get current timestamp in ISO 8601 format
    let now = chrono::Utc::now().to_rfc3339();

    // Check if project already exists
    if let Some(existing) = projects.iter_mut().find(|p| p.path == path) {
        // Update existing project
        existing.last_accessed = now;
        existing.access_count += 1;
    } else {
        // Add new project
        let new_project = RecentProject {
            path: path.to_string(),
            name,
            last_accessed: now,
            access_count: 1,
        };
        projects.push(new_project);
    }

    // Sort by last_accessed (most recent first)
    projects.sort_by(|a, b| b.last_accessed.cmp(&a.last_accessed));

    // Limit to MAX_RECENT_PROJECTS
    if projects.len() > MAX_RECENT_PROJECTS {
        projects.truncate(MAX_RECENT_PROJECTS);
    }

    // Save to disk
    save_recent_projects(&projects)?;

    Ok(())
}

/// Remove a project from recent projects list
pub fn remove_recent_project(path: &str) -> Result<(), String> {
    // Load existing projects
    let mut projects = load_recent_projects()?;

    // Find and remove the project
    let original_len = projects.len();
    projects.retain(|p| p.path != path);

    if projects.len() == original_len {
        return Err(format!("Project not found in recent list: {}", path));
    }

    // Save to disk
    save_recent_projects(&projects)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recent_project_serialization() {
        let project = RecentProject {
            path: "/home/user/project".to_string(),
            name: "project".to_string(),
            last_accessed: "2025-01-15T10:30:00Z".to_string(),
            access_count: 5,
        };

        let json = serde_json::to_string(&project).unwrap();
        let deserialized: RecentProject = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.path, project.path);
        assert_eq!(deserialized.name, project.name);
        assert_eq!(deserialized.access_count, project.access_count);
    }
}
