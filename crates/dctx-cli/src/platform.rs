use std::io::Write;
use std::process::{Command, Stdio};

pub fn copy_to_clipboard(content: &str) -> Result<(), String> {
    let (cmd, args) = if cfg!(target_os = "macos") {
        ("pbcopy", vec![])
    } else if cfg!(target_os = "windows") {
        ("clip.exe", vec![]) // Windows typically implies PowerShell/CMD availability of clip
    } else if cfg!(target_os = "linux") {
        // Try wl-copy first (Wayland), then xclip (X11)
        // This is a naive check; a more robust one checks env vars, but this suffices for now.
        if is_command_available("wl-copy") {
            ("wl-copy", vec![])
        } else if is_command_available("xclip") {
            ("xclip", vec!["-selection", "clipboard"])
        } else {
            return Err(
                "Clipboard utility not found. Please install 'xclip' or 'wl-copy'.".to_string(),
            );
        }
    } else {
        return Err("Clipboard not supported on this OS.".to_string());
    };

    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn clipboard command '{}': {}", cmd, e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(content.as_bytes())
            .map_err(|e| format!("Failed to write to clipboard stdin: {}", e))?;
    }

    let status = child
        .wait()
        .map_err(|e| format!("Failed to wait on clipboard command: {}", e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "Clipboard command '{}' returned non-zero exit code.",
            cmd
        ))
    }
}

fn is_command_available(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}
