<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Rust Code Style Guide

**Status:** Active
**Purpose:** This document defines the standards and best practices for writing Rust code. The goal is to ensure that all code is consistent, readable, idiomatic, and robust. Adherence to these rules is crucial for the long-term maintainability of projects using this technology stack.

---

## **1. Code Quality Tools**

Our development process relies on two fundamental tools from the Rust ecosystem to automate and standardize code quality.

### **1.1. Formatting: `rustfmt`**

All discussions about style (spacing, line breaks, indentation) are eliminated by using the official Rust formatter.

*   **Tool:** `rustfmt`
*   **Configuration:** All rules are centralized in a `rustfmt.toml` file in the project root.
*   **Rule:** Before every `git commit`, the developer is required to run `cargo fmt --all` to ensure the code is formatted according to the standard. The CI pipeline will check formatting and reject changes that do not comply with the rules.

### **1.2. Linting (Static Analysis): `clippy`**

Clippy is our "virtual senior developer" that finds common errors, non-idiomatic code, potential performance issues, and suggests improvements.

*   **Tool:** `clippy`
*   **Configuration:** Rules are centrally defined in the workspace `Cargo.toml` file (`[workspace.lints.clippy]`).
*   **Rule:** Our CI pipeline runs `cargo clippy` with a flag that treats all warnings as errors. The developer is required to run `cargo clippy --workspace` locally to fix all reported issues before submitting code for review.

---

## **2. Idiomatic Code Patterns**

These are key patterns that `clippy` often warns about, and adopting them makes code cleaner, safer, and easier to read.

### **2.1. Iterators Instead of C-style Loops**

Rust's iterator system is powerful and safe. Manual index management is error-prone and considered non-idiomatic.

*   **Problem:** Using `for i in 0..vec.len() { vec[i] }`.
*   **Solution:** Use iterators directly.

*   **Bad Example (avoid):**
    ```rust
    let items = vec!["a", "b", "c"];
    for i in 0..items.len() {
        println!("Item: {}", items[i]);
    }
    ```

*   **Good Example:**
    ```rust
    let items = vec!["a", "b", "c"];

    // If you don't need the index:
    for item in &items {
        println!("Item: {}", item);
    }

    // If you need both index and value:
    for (i, item) in items.iter().enumerate() {
        println!("Item at index {}: {}", i, item);
    }
    ```

### **2.2. Elegant Error Handling Within Loops**

When an iterator returns `Result` or `Option`, avoid manual checking with `if let`.

*   **Problem:** `for result in iterator { if let Ok(value) = result { ... } }`.
*   **Solution:** Use the `.flatten()` method, which automatically filters out all `Err` or `None` variants.

*   **Bad Example (avoid):**
    ```rust
    let results = vec![Ok(1), Err("error"), Ok(3)];
    for result in results {
        if let Ok(value) = result {
            println!("Value: {}", value);
        }
    }
    ```

*   **Good Example:**
    ```rust
    let results = vec![Ok(1), Err("error"), Ok(3)];
    for value in results.into_iter().flatten() {
        println!("Value: {}", value);
    }
    ```

### **2.3. Functional Methods for `Option` and `Result`**

Avoid manually writing `match` or `if let` blocks when there is a built-in functional method that does the same thing.

*   **Problem:** Manual implementation of `map` logic.
*   **Solution:** Use `.map()`, `.and_then()`, `.is_some_and()`, `.unwrap_or()` etc.

*   **Bad Example (avoid):**
    ```rust
    let maybe_number: Option<i32> = Some(5);
    let maybe_incremented = if let Some(n) = maybe_number {
        Some(n + 1)
    } else {
        None
    };
    ```

*   **Good Example:**
    ```rust
    let maybe_number: Option<i32> = Some(5);
    let maybe_incremented = maybe_number.map(|n| n + 1);
    ```

### **2.4. Struct Design and the `Default` Trait**

It is standard convention for structs that can be created without arguments to implement the `Default` trait.

*   **Problem:** Having `fn new() -> Self` without implementing `Default`.
*   **Solution:** Implement `Default` and let `new()` be just an alias for `default()`.

*   **Bad Example (avoid):**
    ```rust
    pub struct Config;
    impl Config {
        pub fn new() -> Self { Config }
    }
    ```

*   **Good Example:**
    ```rust
    pub struct Config;

    impl Default for Config {
        fn default() -> Self {
            Config
        }
    }

    impl Config {
        pub fn new() -> Self {
            Self::default()
        }
    }
    ```

---

## **3. Error Handling: No Panics!**

Panic (`panic!`) causes the program to crash and should only be used in situations where recovery is impossible (e.g., in tests to verify invalid state).

*   **Rule:** Using `.unwrap()` and `.expect()` is **strictly forbidden** in production code (libraries and applications).
*   **Rationale:** Robust software must handle errors, not crash.
*   **Correct approach:** Use the `?` operator to propagate errors up the call stack and `match` or `if let Err(...)` to handle errors where they can be processed.

*   **Bad Example (Forbidden):**
    ```rust
    let content = std::fs::read_to_string("config.toml").unwrap();
    ```

*   **Good Example (within a function that returns `Result`):**
    ```rust
    fn read_config() -> Result<String, std::io::Error> {
        let content = std::fs::read_to_string("config.toml")?;
        Ok(content)
    }
    ```
