// crates/dctx-cli/src/tui.rs
//
// Zero-Dependency Terminal User Interface for dctx
//
// Uses raw terminal control without external dependencies like crossterm or tui-rs.
// Provides a simple, fast menu system for interactive use.

use std::io::{self, Write};

/// Terminal control codes
mod term {
    // Clear screen and move cursor to top-left
    pub const CLEAR_SCREEN: &str = "\x1B[2J\x1B[H";

    // Colors
    pub const RESET: &str = "\x1B[0m";
    pub const BOLD: &str = "\x1B[1m";
    pub const DIM: &str = "\x1B[2m";
    pub const CYAN: &str = "\x1B[36m";
    pub const GREEN: &str = "\x1B[32m";
    pub const YELLOW: &str = "\x1B[33m";
    pub const MAGENTA: &str = "\x1B[35m";
}

/// Menu item definition
pub struct MenuItem {
    pub key: char,
    pub label: String,
    pub description: String,
}

impl MenuItem {
    pub fn new(key: char, label: &str, description: &str) -> Self {
        Self {
            key,
            label: label.to_string(),
            description: description.to_string(),
        }
    }
}

/// Menu configuration
pub struct Menu {
    pub title: String,
    pub items: Vec<MenuItem>,
    pub footer: Option<String>,
}

impl Menu {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            items: Vec::new(),
            footer: None,
        }
    }

    pub fn add_item(&mut self, key: char, label: &str, description: &str) -> &mut Self {
        self.items.push(MenuItem::new(key, label, description));
        self
    }

    pub fn set_footer(&mut self, footer: &str) -> &mut Self {
        self.footer = Some(footer.to_string());
        self
    }

    /// Display the menu and return the selected key
    pub fn show(&self) -> io::Result<char> {
        // Clear screen and display menu
        print!("{}", term::CLEAR_SCREEN);

        // Print header
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

        // Print menu items
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

        // Print footer
        if let Some(footer) = &self.footer {
            println!("{}{}{}{}", term::DIM, term::YELLOW, footer, term::RESET);
        }

        // Prompt for input
        print!("\n{}{}> {}", term::BOLD, term::MAGENTA, term::RESET);
        io::stdout().flush()?;

        // Read single character
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        // Return first character (trimmed)
        Ok(input.trim().chars().next().unwrap_or('\0'))
    }
}

/// Context information displayed in menus
pub struct MenuContext {
    pub current_dir: String,
    pub last_version: Option<String>,
}

impl MenuContext {
    pub fn new(current_dir: String) -> Self {
        Self {
            current_dir,
            last_version: None,
        }
    }

    pub fn with_version(mut self, version: String) -> Self {
        self.last_version = Some(version);
        self
    }

    pub fn format(&self) -> String {
        let mut parts = vec![format!("Directory: {}", self.current_dir)];

        if let Some(version) = &self.last_version {
            parts.push(format!("Last Snapshot: {}", version));
        } else {
            parts.push("Last Snapshot: None (fresh start)".to_string());
        }

        parts.join(" | ")
    }
}

/// Build the main menu
pub fn create_main_menu(context: &MenuContext) -> Menu {
    let mut menu = Menu::new("dctx - Directory Context Generator");

    menu.add_item(
        '1',
        "Snapshot Current Directory",
        "Generate full context snapshot",
    )
    .add_item(
        '2',
        "Snapshot to Clipboard",
        "Generate and copy to clipboard",
    )
    .add_item('3', "Tree-Only Mode", "Show directory structure only")
    .add_item('4', "Diff Mode", "Generate diff from last snapshot")
    .add_item('5', "Advanced Options →", "Filters, templates, and cache")
    .add_item('0', "Exit", "")
    .set_footer(&context.format());

    menu
}

/// Build the advanced menu
pub fn create_advanced_menu() -> Menu {
    let mut menu = Menu::new("Advanced Options");

    menu.add_item('1', "Filters →", "Extension, size, content filters")
        .add_item('2', "Templates →", "Manage output templates")
        .add_item('3', "Cache & Debug →", "State management and diagnostics")
        .add_item('0', "← Back to Main Menu", "")
        .set_footer("Escape or 0 to go back");

    menu
}

/// Build the filter submenu
pub fn create_filter_menu() -> Menu {
    let mut menu = Menu::new("Filter Options");

    menu.add_item('1', "Filter by Extension", "e.g., rs,toml,md")
        .add_item('2', "Filter by Size", "Large or small files")
        .add_item('3', "Content Filters", "Empty or non-empty files")
        .add_item('4', "Hidden Files Only", "Show .hidden files")
        .add_item(
            '5',
            "Include Binary Files",
            "Include binary files in output",
        )
        .add_item('9', "Clear All Filters", "")
        .add_item('0', "← Back", "")
        .set_footer("Filters combine for precise control");

    menu
}

/// Build the template submenu
pub fn create_template_menu() -> Menu {
    let mut menu = Menu::new("Template Management");

    menu.add_item('1', "Use Default Template", "Standard dctx output format")
        .add_item('2', "List Available Templates", "Show compiled templates")
        .add_item('3', "Build New Template", "Compile a .dctx_template file")
        .add_item('0', "← Back", "")
        .set_footer("Templates customize output format");

    menu
}

/// Build the cache/debug submenu
pub fn create_cache_menu() -> Menu {
    let mut menu = Menu::new("Cache & Debug");

    menu.add_item('1', "Show Ignored Folders", "List .gitignore exclusions")
        .add_item('2', "Reset Project State", "Force V1 reset")
        .add_item('3', "Clear Global Cache", "Remove all cached states")
        .add_item('0', "← Back", "")
        .set_footer("Manage cached state and diagnostics");

    menu
}

/// Get input string from user
pub fn get_input(prompt: &str) -> io::Result<String> {
    print!("\n{}{}{} ", term::BOLD, prompt, term::RESET);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    Ok(input.trim().to_string())
}

/// Show a message and wait for Enter
pub fn show_message(message: &str) -> io::Result<()> {
    println!("\n{}{}{}", term::GREEN, message, term::RESET);
    println!("\n{}Press Enter to continue...{}", term::DIM, term::RESET);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    Ok(())
}

/// Show an error message and wait for Enter
pub fn show_error(message: &str) -> io::Result<()> {
    println!("\n{}{}{}", term::YELLOW, message, term::RESET);
    println!("\n{}Press Enter to continue...{}", term::DIM, term::RESET);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_menu_item_new() {
        let item = MenuItem::new('1', "Test Label", "Test Description");
        assert_eq!(item.key, '1');
        assert_eq!(item.label, "Test Label");
        assert_eq!(item.description, "Test Description");
    }

    #[test]
    fn test_menu_item_new_empty_description() {
        let item = MenuItem::new('a', "Label Only", "");
        assert_eq!(item.key, 'a');
        assert_eq!(item.label, "Label Only");
        assert_eq!(item.description, "");
    }

    #[test]
    fn test_menu_new() {
        let menu = Menu::new("Test Menu");
        assert_eq!(menu.title, "Test Menu");
        assert_eq!(menu.items.len(), 0);
        assert_eq!(menu.footer, None);
    }

    #[test]
    fn test_menu_add_item() {
        let mut menu = Menu::new("Test");
        menu.add_item('1', "Option 1", "First option");

        assert_eq!(menu.items.len(), 1);
        assert_eq!(menu.items[0].key, '1');
        assert_eq!(menu.items[0].label, "Option 1");
        assert_eq!(menu.items[0].description, "First option");
    }

    #[test]
    fn test_menu_add_multiple_items() {
        let mut menu = Menu::new("Test");
        menu.add_item('1', "Option 1", "First")
            .add_item('2', "Option 2", "Second")
            .add_item('3', "Option 3", "Third");

        assert_eq!(menu.items.len(), 3);
        assert_eq!(menu.items[1].key, '2');
        assert_eq!(menu.items[2].label, "Option 3");
    }

    #[test]
    fn test_menu_set_footer() {
        let mut menu = Menu::new("Test");
        menu.set_footer("Press 0 to exit");

        assert_eq!(menu.footer, Some("Press 0 to exit".to_string()));
    }

    #[test]
    fn test_menu_builder_pattern() {
        let mut menu = Menu::new("Test");
        menu.add_item('1', "First", "")
            .add_item('2', "Second", "")
            .set_footer("Footer text");

        assert_eq!(menu.items.len(), 2);
        assert_eq!(menu.footer.as_deref(), Some("Footer text"));
    }

    #[test]
    fn test_menu_context_new() {
        let context = MenuContext::new("/tmp/test".to_string());
        assert_eq!(context.current_dir, "/tmp/test");
        assert_eq!(context.last_version, None);
    }

    #[test]
    fn test_menu_context_with_version() {
        let context = MenuContext::new("/tmp/test".to_string()).with_version("V2".to_string());

        assert_eq!(context.current_dir, "/tmp/test");
        assert_eq!(context.last_version, Some("V2".to_string()));
    }

    #[test]
    fn test_menu_context_format_no_version() {
        let context = MenuContext::new("/home/user/project".to_string());
        let formatted = context.format();

        assert!(formatted.contains("Directory: /home/user/project"));
        assert!(formatted.contains("Last Snapshot: None (fresh start)"));
        assert!(formatted.contains(" | "));
    }

    #[test]
    fn test_menu_context_format_with_version() {
        let context =
            MenuContext::new("/home/user/project".to_string()).with_version("V5".to_string());
        let formatted = context.format();

        assert!(formatted.contains("Directory: /home/user/project"));
        assert!(formatted.contains("Last Snapshot: V5"));
        assert!(!formatted.contains("fresh start"));
    }

    #[test]
    fn test_create_main_menu() {
        let context = MenuContext::new("/tmp".to_string());
        let menu = create_main_menu(&context);

        assert_eq!(menu.title, "dctx - Directory Context Generator");
        assert_eq!(menu.items.len(), 6); // Options 1-5 + 0 (Exit)
        assert!(menu.footer.is_some());

        // Check specific options exist
        assert!(menu.items.iter().any(|item| item.key == '1'));
        assert!(menu.items.iter().any(|item| item.key == '5'));
        assert!(menu.items.iter().any(|item| item.key == '0'));
    }

    #[test]
    fn test_create_advanced_menu() {
        let menu = create_advanced_menu();

        assert_eq!(menu.title, "Advanced Options");
        assert_eq!(menu.items.len(), 4); // Options 1-3 + 0 (Back)
        assert_eq!(menu.footer, Some("Escape or 0 to go back".to_string()));

        // Verify menu items
        assert!(menu.items.iter().any(|item| item.label.contains("Filters")));
        assert!(menu
            .items
            .iter()
            .any(|item| item.label.contains("Templates")));
        assert!(menu.items.iter().any(|item| item.label.contains("Cache")));
    }

    #[test]
    fn test_create_filter_menu() {
        let menu = create_filter_menu();

        assert_eq!(menu.title, "Filter Options");
        assert_eq!(menu.items.len(), 7); // Options 1-5 + 9 (Clear) + 0 (Back)

        // Check key options
        assert!(menu
            .items
            .iter()
            .any(|item| item.key == '1' && item.label.contains("Extension")));
        assert!(menu
            .items
            .iter()
            .any(|item| item.key == '2' && item.label.contains("Size")));
        assert!(menu
            .items
            .iter()
            .any(|item| item.key == '4' && item.label.contains("Hidden")));
        assert!(menu
            .items
            .iter()
            .any(|item| item.key == '9' && item.label.contains("Clear")));
    }

    #[test]
    fn test_create_template_menu() {
        let menu = create_template_menu();

        assert_eq!(menu.title, "Template Management");
        assert_eq!(menu.items.len(), 4); // Options 1-3 + 0 (Back)

        assert!(menu.items.iter().any(|item| item.label.contains("Default")));
        assert!(menu.items.iter().any(|item| item.label.contains("List")));
        assert!(menu.items.iter().any(|item| item.label.contains("Build")));
    }

    #[test]
    fn test_create_cache_menu() {
        let menu = create_cache_menu();

        assert_eq!(menu.title, "Cache & Debug");
        assert_eq!(menu.items.len(), 4); // Options 1-3 + 0 (Back)

        assert!(menu.items.iter().any(|item| item.label.contains("Ignored")));
        assert!(menu.items.iter().any(|item| item.label.contains("Reset")));
        assert!(menu.items.iter().any(|item| item.label.contains("Clear")));
    }

    #[test]
    fn test_all_menus_have_back_or_exit() {
        // Every menu should have a '0' option to exit or go back
        let context = MenuContext::new("/tmp".to_string());

        let main = create_main_menu(&context);
        let advanced = create_advanced_menu();
        let filter = create_filter_menu();
        let template = create_template_menu();
        let cache = create_cache_menu();

        assert!(main.items.iter().any(|item| item.key == '0'));
        assert!(advanced.items.iter().any(|item| item.key == '0'));
        assert!(filter.items.iter().any(|item| item.key == '0'));
        assert!(template.items.iter().any(|item| item.key == '0'));
        assert!(cache.items.iter().any(|item| item.key == '0'));
    }

    #[test]
    fn test_terminal_control_codes_valid() {
        // Verify ANSI escape codes are correct
        assert_eq!(term::CLEAR_SCREEN, "\x1B[2J\x1B[H");
        assert_eq!(term::RESET, "\x1B[0m");
        assert_eq!(term::BOLD, "\x1B[1m");
        assert_eq!(term::CYAN, "\x1B[36m");
        assert_eq!(term::GREEN, "\x1B[32m");
        assert_eq!(term::YELLOW, "\x1B[33m");
        assert_eq!(term::MAGENTA, "\x1B[35m");
    }
}
