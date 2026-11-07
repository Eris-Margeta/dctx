// SPDX-License-Identifier: Apache-2.0
//! Cache command persistence through the library surface.
//! Implements ARCHITECTURE-PROPOSAL §4 `cache config` and corrupt-ledger refusal.
//! Depends on `dctx-cli` [`run_in`]. The binary is not invoked.

use std::path::PathBuf;

use dctx_cli::run_in;

#[test]
fn config_persists_across_two_invocations() {
    let root = scratch("config");
    let set = run_in(
        &args(&[
            "cache",
            "config",
            "auto",
            "off",
            "--max-bytes",
            "1000000",
            "--ttl-days",
            "3",
        ]),
        Some(&root),
    )
    .expect("set");
    assert!(set.contains("auto: off"));
    assert!(set.contains("max_bytes: 1000000"));
    assert!(set.contains("ttl_days: 3"));
    let first = run_in(&args(&["cache", "config"]), Some(&root)).expect("first");
    let second = run_in(&args(&["cache", "config"]), Some(&root)).expect("second");
    assert_eq!(first, set);
    assert_eq!(second, first);
}

#[test]
fn a_corrupt_ledger_is_reported_and_left_in_place() {
    let root = scratch("corrupt");
    std::fs::write(root.join("ledger.v1"), b"not-a-ledger").expect("write");
    let err = run_in(&args(&["cache", "status"]), Some(&root)).expect_err("status");
    assert!(err.contains("corrupt ledger"), "{err}");
    assert_eq!(
        std::fs::read(root.join("ledger.v1")).expect("kept"),
        b"not-a-ledger"
    );
}

#[test]
fn help_names_config_auto_and_the_ttl_defaults() {
    let page = run_in(&args(&["cache", "--help"]), None).expect("help");
    assert!(page.contains("dctx cache config"));
    assert!(page.contains("dctx cache auto"));
    assert!(page.contains("--no-auto-clean"));
    assert!(page.contains("7 days"));
    assert!(page.contains("cache cap:"));
}

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("dctx-cli-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("dir");
    path
}

mod menu_contract {
    use dctx_cli::tui::*;

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
    }

    #[test]
    fn test_menu_context_format_no_version() {
        let context = MenuContext::new("/home/user/project".to_string());
        let formatted = context.format();

        assert!(formatted.contains("Directory: /home/user/project"));
    }

    #[test]
    fn test_create_main_menu() {
        let context = MenuContext::new("/tmp".to_string());
        let menu = create_main_menu(&context);

        assert_eq!(menu.title, "dctx - Directory Context Generator");
        assert_eq!(menu.items.len(), 5); // Options 1-4 + 0 (Exit)
        assert!(menu.footer.is_some());

        // Check specific options exist
        assert!(menu.items.iter().any(|item| item.key == '1'));
        assert!(menu.items.iter().any(|item| item.key == '4'));
        assert!(menu.items.iter().any(|item| item.key == '0'));
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

        assert_eq!(menu.title, "Cache");
        assert_eq!(menu.items.len(), 4); // Options 1-3 + 0 (Back)

        assert!(menu.items.iter().any(|item| item.label.contains("Status")));
        assert!(menu.items.iter().any(|item| item.label.contains("Preview")));
        assert!(menu.items.iter().any(|item| item.label.contains("Collect")));
    }

    #[test]
    fn test_all_menus_have_back_or_exit() {
        // Every menu should have a '0' option to exit or go back
        let context = MenuContext::new("/tmp".to_string());

        let main = create_main_menu(&context);
        let template = create_template_menu();
        let cache = create_cache_menu();

        assert!(main.items.iter().any(|item| item.key == '0'));
        assert!(template.items.iter().any(|item| item.key == '0'));
        assert!(cache.items.iter().any(|item| item.key == '0'));
    }
}
