// SPDX-License-Identifier: Apache-2.0
//! Terminal menus for explicit pack, template and managed cache commands.
//! Menu copy lives in help; terminal IO uses std without child processes.

pub use crate::help::{create_cache_menu, create_main_menu, create_template_menu};
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
    /// Input character that selects this row.
    pub key: char,
    /// Primary text displayed beside the selection key.
    pub label: String,
    /// Optional explanatory text; an empty string hides the description.
    pub description: String,
}
impl MenuItem {
    /// Creates a menu row by copying its label and description.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn new(key: char, label: &str, description: &str) -> Self {
        Self {
            key,
            label: label.to_string(),
            description: description.to_string(),
        }
    }
}

/// Menu presentation, independent of application actions.
pub struct Menu {
    /// Heading displayed above the menu rows.
    pub title: String,
    /// Selectable rows in display order.
    pub items: Vec<MenuItem>,
    /// Optional text displayed below all rows.
    pub footer: Option<String>,
}
impl Menu {
    /// Creates an empty menu with the supplied title.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            items: Vec::new(),
            footer: None,
        }
    }
    /// Appends a selectable row and returns this menu for chaining.
    pub fn add_item(&mut self, key: char, label: &str, description: &str) -> &mut Self {
        self.items.push(MenuItem::new(key, label, description));
        self
    }
    /// Replaces the footer text and returns this menu for chaining.
    pub fn set_footer(&mut self, footer: &str) -> &mut Self {
        self.footer = Some(footer.to_string());
        self
    }
    /// Displays the menu and reads the first trimmed input character.
    /// Returns the null character for an empty input line or end of input.
    ///
    /// # Errors
    /// Returns an IO error if flushing stdout or reading stdin fails.
    pub fn show(&self) -> io::Result<char> {
        print!("{}", term::CLEAR_SCREEN);
        println!(
            "{}{}═══════════════════════════════════════════════════════════{}",
            term::BOLD,
            term::CYAN,
            term::RESET
        );
        println!(
            "{}{}  {}  {}",
            term::BOLD,
            term::CYAN,
            self.title,
            term::RESET
        );
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

/// Directory displayed in the main menu.
pub struct MenuContext {
    /// Directory label shown to the user.
    pub current_dir: String,
}
impl MenuContext {
    /// Creates context for a directory.
    #[must_use]
    pub fn new(current_dir: String) -> Self {
        Self { current_dir }
    }
    /// Formats the directory label for display.
    #[must_use]
    pub fn format(&self) -> String {
        format!("Directory: {}", self.current_dir)
    }
}

/// Displays a prompt and reads a trimmed line from stdin.
///
/// # Errors
/// Returns an IO error if flushing stdout or reading stdin fails.
pub fn get_input(prompt: &str) -> io::Result<String> {
    print!("\n{}{}{} ", term::BOLD, prompt, term::RESET);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

/// Displays a success message and waits for an input line.
///
/// # Errors
/// Returns an IO error if flushing stdout or reading stdin fails.
pub fn show_message(message: &str) -> io::Result<()> {
    notice(message, term::GREEN)
}
/// Displays an error message and waits for an input line.
///
/// # Errors
/// Returns an IO error if flushing stdout or reading stdin fails.
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

/// Run the interactive command loop through the explicit command handlers.
pub fn run() {
    let current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let context = MenuContext::new(current.display().to_string());
    loop {
        let result = match create_main_menu(&context).show() {
            Ok('1') => {
                crate::pack::run(&["pack".into(), ".".into()]).map_err(|error| error.render())
            }
            Ok('2') => crate::pack::run(&["pack".into(), ".".into(), "--clipboard".into()])
                .map_err(|error| error.render()),
            Ok('3') => {
                crate::template::run_menu();
                continue;
            }
            Ok('4') => {
                cache_menu();
                continue;
            }
            Ok('0' | 'q' | '\0') => return,
            Ok(_) => Err("Invalid selection. Try again.".into()),
            Err(error) => {
                eprintln!("Error reading input: {error}");
                return;
            }
        };
        match result {
            Ok(()) => {
                let _ = show_message("Pack complete.");
            }
            Err(error) => {
                let _ = show_error(&error);
            }
        }
    }
}

fn cache_menu() {
    loop {
        let args: &[&str] = match create_cache_menu().show() {
            Ok('1') => &["cache", "status"],
            Ok('2') => &["cache", "gc", "--dry-run"],
            Ok('3') => &["cache", "gc"],
            Ok('0' | '\0') => return,
            Ok(_) => {
                let _ = show_error("Invalid selection. Try again.");
                continue;
            }
            Err(error) => {
                eprintln!("Error reading input: {error}");
                return;
            }
        };
        let args: Vec<_> = args.iter().map(|arg| (*arg).to_string()).collect();
        match crate::cache::run(&args) {
            Ok(()) => {
                let _ = show_message("Cache command complete.");
            }
            Err(error) => {
                let _ = show_error(&error);
            }
        }
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
