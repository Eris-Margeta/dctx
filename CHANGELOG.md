# Changelog

All notable changes to the `dircontxt` project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2025-11-15

This is the first official public release of `dircontxt`. This version marks a stable, feature-complete tool for creating intelligent, version-aware project snapshots for Large Language Models.

### Added

-   **Core Snapshot Generation**:
    -   Implemented the primary functionality to recursively scan a directory and capture its complete file and folder structure.

-   **Dual-Output System**:
    -   **LLM-Optimized Text File (`.llmcontext.txt`)**: Generates a structured text file with a clear manifest and separated file content blocks, designed for optimal parsing by AI models. Includes unique IDs for every file and directory to allow for precise navigation.
    -   **Binary State File (`.dircontxt`)**: Creates a compact binary archive that serves as a persistent, machine-readable state file for the project snapshot.

-   **Automatic Versioning System**:
    -   The application is now state-aware. On subsequent runs, it reads the previously generated text file to determine the version.
    -   Automatically increments versions with each new snapshot (e.g., `V1` -> `V1.1` -> `V1.2`).
    -   Introduced a new `version.c` module to handle version parsing and calculation.

-   **Diff Generation**:
    -   When changes are detected between runs, the tool automatically generates a concise diff file (`-diff.txt`).
    -   The diff file includes a summary of all added, modified, and removed files, the updated directory tree, and the full content of only the changed files.
    -   Added a new `diff.c` module to perform tree comparison logic.

-   **Advanced Ignore System**:
    -   Implemented a powerful three-tiered file-ignoring hierarchy:
        1.  **Hardcoded Defaults**: Core patterns like `.git/` and `node_modules/` are ignored by default.
        2.  **Global Ignore File**: Supports a user-wide ignore file at `~/.config/dircontxt/ignore`.
        3.  **Project-Specific Ignore File**: Supports a `.dircontxtignore` file in the target directory for the highest level of control.
    -   The ignore parser supports `.gitignore`-like syntax, including comments (`#`), directory-only patterns (`build/`), suffix wildcards (`*.log`), and negation (`!important.log`).

-   **Global Configuration File**:
    -   The program now reads a global configuration file from `~/.config/dircontxt/config`.
    -   Added the `OUTPUT_MODE` setting to control whether the user-facing text files are generated (`both` or `binary`).
    -   Created a new `config.c` module to manage loading and parsing settings.

-   **Smart Binary File Detection**:
    -   The formatter intelligently detects binary files by both file extension and content analysis (checking for null bytes and non-printable characters).
    -   Binary file content is replaced with a single-line placeholder in the text output, dramatically saving tokens and improving readability.

-   **Robust Build System & Documentation**:
    -   Created a comprehensive `Makefile` with targets for debug (`make all`), optimized release (`make release`), testing (`make test`), and cleaning (`make clean`).
    -   Wrote a detailed `README.md` explaining all features, installation steps for macOS and Debian/Ubuntu, and advanced usage.
