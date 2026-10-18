//! Terminal menus and snapshot actions, per D3-LEGACY-REMOVAL §5 D3-04.
//! Uses the same compatibility host as the alias and managed cache requests for purge.
//! Menu copy lives in help; terminal IO uses std without child processes.

pub use crate::help::{
    create_advanced_menu, create_cache_menu, create_filter_menu, create_main_menu, create_template_menu,
};
use crate::{compat, run_headless_mode, SnapshotConfig};
use dctx_core::filter::FilterConfig;
use std::{
    io::{self, Write},
    path::PathBuf,
};

mod term {
    pub const CLEAR_SCREEN: &str = "\x1B[2J\x1B[H";
    pub const RESET: &str = "\x1B[0m";
    pub const BOLD: &str = "\x1B[1m";
    pub const DIM: &str = "\x1B[2m";
    pub const CYAN: &str = "\x1B[36m";
    pub const GREEN: &str = "\x1B[32m";
    pub const YELLOW: &str = "\x1B[33m";
    pub const MAGENTA: &str = "\x1B[35m";
}

/// One selectable menu row.
pub struct MenuItem {
    pub key: char,
    pub label: String,
    pub description: String,
}
impl MenuItem {
    pub fn new(key: char, label: &str, description: &str) -> Self {
        Self { key, label: label.to_string(), description: description.to_string() }
    }
}

/// Menu presentation, independent of application actions.
pub struct Menu {
    pub title: String,
    pub items: Vec<MenuItem>,
    pub footer: Option<String>,
}
impl Menu {
    pub fn new(title: &str) -> Self {
        Self { title: title.to_string(), items: Vec::new(), footer: None }
    }
    pub fn add_item(&mut self, key: char, label: &str, description: &str) -> &mut Self {
        self.items.push(MenuItem::new(key, label, description));
        self
    }
    pub fn set_footer(&mut self, footer: &str) -> &mut Self {
        self.footer = Some(footer.to_string());
        self
    }
    pub fn show(&self) -> io::Result<char> {
        print!("{}", term::CLEAR_SCREEN);
        println!(
            "{}{}═══════════════════════════════════════════════════════════{}",
            term::BOLD,
            term::CYAN,
            term::RESET
        );
        println!("{}{}  {}  {}", term::BOLD, term::CYAN, self.title, term::RESET);
        println!(
            "{}{}═══════════════════════════════════════════════════════════{}\n",
            term::BOLD,
            term::CYAN,
            term::RESET
        );
        for item in &self.items {
            println!(
                "  {}{}[{}]{} {}{}{}",
                term::GREEN,
                term::BOLD,
                item.key,
                term::RESET,
                term::BOLD,
                item.label,
                term::RESET
            );
            if !item.description.is_empty() {
                println!("      {}{}{}", term::DIM, item.description, term::RESET);
            }
            println!();
        }
        if let Some(footer) = &self.footer {
            println!("{}{}{}{}", term::DIM, term::YELLOW, footer, term::RESET);
        }
        print!("\n{}{}> {}", term::BOLD, term::MAGENTA, term::RESET);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(input.trim().chars().next().unwrap_or('\0'))
    }
}

/// Directory and last legacy snapshot revision displayed in the main menu.
pub struct MenuContext {
    pub current_dir: String,
    pub last_version: Option<String>,
}
impl MenuContext {
    pub fn new(current_dir: String) -> Self {
        Self { current_dir, last_version: None }
    }
    pub fn with_version(mut self, version: String) -> Self {
        self.last_version = Some(version);
        self
    }
    pub fn format(&self) -> String {
        format!(
            "Directory: {} | Last Snapshot: {}",
            self.current_dir,
            self.last_version.as_deref().unwrap_or("None (fresh start)")
        )
    }
}

pub fn get_input(prompt: &str) -> io::Result<String> {
    print!("\n{}{}{} ", term::BOLD, prompt, term::RESET);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

pub fn show_message(message: &str) -> io::Result<()> {
    notice(message, term::GREEN)
}
pub fn show_error(message: &str) -> io::Result<()> {
    notice(message, term::YELLOW)
}
fn notice(message: &str, color: &str) -> io::Result<()> {
    println!("\n{}{}{}", color, message, term::RESET);
    println!("\n{}Press Enter to continue...{}", term::DIM, term::RESET);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(())
}

/// Run the interactive command loop using the alias's shared snapshot operation.
pub fn run() {
    let current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let context =
        MenuContext { current_dir: current.display().to_string(), last_version: compat::last_version(&current) };
    loop {
        let key = match create_main_menu(&context).show() {
            Ok(key) => key,
            Err(error) => {
                eprintln!("Error reading input: {error}");
                std::process::exit(1);
            }
        };
        let (config, message) = match key {
            '1' => (SnapshotConfig::default(), "Snapshot complete! Check .dctx/ directory."),
            '2' => (SnapshotConfig { clipboard_mode: true, ..Default::default() }, "Snapshot copied to clipboard!"),
            '3' => (SnapshotConfig { tree_only: true, ..Default::default() }, "Tree-only snapshot complete!"),
            '4' => (
                SnapshotConfig { clipboard_mode: true, diff_mode: true, ..Default::default() },
                "Diff copied to clipboard!",
            ),
            '5' => {
                advanced();
                continue;
            }
            '0' | 'q' => {
                println!("Goodbye!");
                return;
            }
            _ => {
                let _ = show_error("Invalid selection. Try again.");
                continue;
            }
        };
        run_headless_mode(config);
        let _ = show_message(message);
    }
}

fn advanced() {
    loop {
        match create_advanced_menu().show() {
            Ok('1') => filters(),
            Ok('2') => crate::template::run_menu(),
            Ok('3') => compat::run_cache_menu(),
            Ok('0') => return,
            Ok(_) => {
                let _ = show_error("Invalid selection. Try again.");
            }
            Err(error) => {
                eprintln!("Error reading input: {error}");
                return;
            }
        }
    }
}

fn filters() {
    let mut filter = FilterConfig::default();
    loop {
        let result = match create_filter_menu().show() {
            Ok('1') => get_input("Enter extensions (comma-separated, e.g., rs,toml):").map(|input| {
                let extensions: Vec<_> =
                    input.split(',').map(str::trim).filter(|part| !part.is_empty()).map(str::to_string).collect();
                if !extensions.is_empty() {
                    let _ = show_message(&format!("Extension filter set: {}", extensions.join(", ")));
                    filter.extensions = extensions;
                }
            }),
            Ok('2') => size_filter(&mut filter),
            Ok('3') => {
                get_input("Empty (E) or Non-Empty (N) files?").map(|input| match input.to_lowercase().as_str() {
                    "e" | "empty" => {
                        filter.only_empty = true;
                        let _ = show_message("Empty files filter set.");
                    }
                    "n" | "non-empty" => {
                        filter.only_non_empty = true;
                        let _ = show_message("Non-empty files filter set.");
                    }
                    _ => {}
                })
            }
            Ok('4') => {
                filter.only_hidden = true;
                show_message("Hidden files only filter set.")
            }
            Ok('5') => {
                filter.include_binary = true;
                show_message("Binary files will be included.")
            }
            Ok('9') => {
                filter = FilterConfig::default();
                show_message("All filters cleared.")
            }
            Ok('0') => {
                if filter.has_active_filters() {
                    run_headless_mode(SnapshotConfig { filter_config: filter, ..Default::default() });
                    let _ = show_message("Filtered snapshot generated!");
                }
                return;
            }
            Ok(_) => show_error("Invalid selection. Try again."),
            Err(error) => {
                eprintln!("Error reading input: {error}");
                return;
            }
        };
        if let Err(error) = result {
            let _ = show_error(&format!("Error: {error}"));
        }
    }
}

fn size_filter(filter: &mut FilterConfig) -> io::Result<()> {
    let input = get_input("Large (L) or Small (S) files?")?.to_lowercase();
    let large = matches!(input.as_str(), "l" | "large");
    if !large && !matches!(input.as_str(), "s" | "small") {
        return Ok(());
    }
    let text = get_input(if large { "Minimum size in MB:" } else { "Maximum size in MB:" })?;
    if let Ok(size) = text.parse::<f64>() {
        let bytes = Some((size * 1_048_576.0) as u64);
        if large {
            filter.min_size_bytes = bytes;
        } else {
            filter.max_size_bytes = bytes;
        }
        show_message(&format!(
            "{} file filter set: {}{size:.2}MB",
            if large { "Large" } else { "Small" },
            if large { ">" } else { "<" }
        ))
    } else {
        show_error("Invalid size number.")
    }
}

#[cfg(test)]
mod tests {
    use super::term;
    #[test]
    fn terminal_control_codes_are_unchanged() {
        assert_eq!(term::CLEAR_SCREEN, "\x1B[2J\x1B[H");
        assert_eq!(term::RESET, "\x1B[0m");
        assert_eq!(term::BOLD, "\x1B[1m");
        assert_eq!(term::CYAN, "\x1B[36m");
        assert_eq!(term::GREEN, "\x1B[32m");
        assert_eq!(term::YELLOW, "\x1B[33m");
        assert_eq!(term::MAGENTA, "\x1B[35m");
    }
}
