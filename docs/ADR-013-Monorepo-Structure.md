# ADR-013: Monorepo Structure with Mixed Licensing

**Date:** 2026-01-25
**Status:** Accepted

---

## 1. Context and Problem Statement

As dctx evolves, we need to support multiple distribution formats:
1. **Core library** - For Rust developers
2. **CLI tool** - For command-line users
3. **Desktop GUI** - For non-technical users (commercial product)
4. **Language bindings** - For Python/Node.js developers (future)

Previously, the repository was organized as a simple Rust workspace with two crates. This structure doesn't clearly separate:
- Open-source components (MIT)
- Commercial components (Proprietary)
- Future language bindings
- Distribution artifacts

## 2. Decision Drivers

* **Clear Licensing**: Separate MIT and proprietary code physically
* **Monorepo Benefits**: Single source of truth, shared CI/CD, atomic commits
* **Scalability**: Easy to add bindings without restructuring again
* **Business Model**: Support "Open Core" licensing model
* **Developer Experience**: Obvious where each component lives
* **Distribution**: Multiple distribution formats from one repo
* **Code Reuse**: Desktop app and bindings use same dctx-core

## 3. Considered Options

### Option 1: Multi-Repo (Rejected)

**Structure:**
```
github.com/dctx/dctx-core       (MIT)
github.com/dctx/dctx-cli        (MIT)
github.com/dctx/dctx-desktop    (Proprietary)
github.com/dctx/dctx-py         (MIT)
```

**Pros:**
- Clear separation
- Independent versioning
- Easier to sell desktop app separately

**Cons:**
- Synchronization nightmare (version bumps require 4 PRs)
- No atomic commits across components
- Duplicated CI/CD
- Harder to develop features spanning multiple components
- Breaking changes in core require coordinated releases

**Decision:** Rejected - Overhead too high for solo/small team

### Option 2: Monorepo with Workspaces (Selected)

**Structure:**
```
dctx/
├── crates/        # Rust workspace (MIT)
├── apps/          # Applications (Mixed licensing)
├── bindings/      # Language bindings (MIT)
├── tools/         # Shared tooling
└── docs/          # Documentation
```

**Pros:**
- Single source of truth
- Atomic commits across all components
- Shared CI/CD
- Easy cross-component refactoring
- One issue tracker
- Used by: Turborepo, Next.js, Tauri

**Cons:**
- Slightly more complex structure
- Need clear documentation of licensing

**Decision:** Selected - Best for small team, common in modern projects

## 4. Decision Outcome

**Chosen approach:** Monorepo with clear directory-based licensing

### Directory Structure

```
dctx/
├── crates/                    # MIT Licensed Rust Workspace
│   ├── dctx-core/             # Core library (0 external deps)
│   └── dctx-cli/              # CLI binary
│
├── apps/                      # Applications
│   └── dctx-desktop/          # PROPRIETARY - Tauri GUI ($29)
│       ├── src/               # Frontend (TypeScript)
│       ├── src-tauri/         # Backend (Rust + dctx-core)
│       ├── LICENSE            # PROPRIETARY
│       └── README.md
│
├── bindings/                  # MIT Licensed Language Bindings
│   ├── dctx-py/               # Python (PyO3) - Phase 12
│   └── dctx-node/             # Node.js (NAPI-RS) - Phase 13
│
├── tools/                     # Build and development scripts
├── docs/                      # Documentation
├── website/                   # Landing page
├── .github/                   # CI/CD workflows
│
├── Cargo.toml                 # Workspace definition
├── LICENSE                    # MIT (applies to crates/ and bindings/)
└── README.md                  # Explains licensing model
```

### Licensing Model

**MIT Licensed (Free Forever):**
- `crates/dctx-core/` - Core library
- `crates/dctx-cli/` - CLI with ALL features
- `bindings/dctx-py/` - Python bindings (future)
- `bindings/dctx-node/` - Node.js bindings (future)
- Root `LICENSE` file applies to these

**Proprietary Licensed (Commercial):**
- `apps/dctx-desktop/` - Desktop GUI application
- Contains its own `LICENSE` file (PROPRIETARY)
- $29 one-time payment
- 14-day trial period
- Source code visible for transparency/audit

**Philosophy**: "Open Core" model
- Keep developer tools free and open
- Monetize GUI convenience for non-technical users
- Similar to: GitLab, Sentry, Mattermost

### Workspace Configuration

**Root Cargo.toml:**
```toml
[workspace]
members = [
  "crates/dctx-core",
  "crates/dctx-cli",
  "apps/dctx-desktop/src-tauri",
  # Future bindings
  # "bindings/dctx-py",
  # "bindings/dctx-node",
]
resolver = "2"
```

**Benefits:**
- Single `cargo test` runs all Rust tests
- Single `cargo build --release` builds all Rust components
- Shared `Cargo.lock` for consistent dependencies
- IDE support works across entire workspace

### Migration from Old Structure

**Before:**
```
dctx/
├── crates/
│   ├── dctx-core/
│   └── dctx-cli/
└── ...
```

**After:**
```
dctx/
├── crates/          # Same (no change)
├── apps/            # NEW - Desktop GUI
├── bindings/        # NEW - Future language bindings
└── ...
```

**Migration Steps:**
1. ✅ Create `apps/` directory
2. ✅ Create `bindings/` directory
3. ✅ Create `docs/` directory
4. ✅ Initialize `apps/dctx-desktop/` with Tauri
5. ✅ Add proprietary LICENSE to desktop app
6. ✅ Update root `Cargo.toml` workspace members
7. ✅ Update root README explaining structure
8. ✅ Create placeholder READMEs in `bindings/`

## 5. Positive Consequences

* ✅ **Clear Licensing**: Physical separation of MIT vs Proprietary code
* ✅ **Atomic Commits**: Cross-component changes in single commit
* ✅ **Shared CI/CD**: One workflow tests everything
* ✅ **Code Reuse**: Desktop app uses dctx-core directly (no duplication)
* ✅ **Scalability**: Easy to add Python/Node.js bindings
* ✅ **Single Version**: All components share version number (0.11.0)
* ✅ **Developer Experience**: Clear structure, obvious where things go
* ✅ **Business Model**: Supports open-core monetization
* ✅ **Transparency**: Desktop app source visible even though proprietary

## 6. Negative Consequences

* ⚠️ **Complexity**: Slightly more complex than simple two-crate workspace
* ⚠️ **Documentation**: Need to clearly explain licensing in README
* ⚠️ **Build Time**: Desktop app adds build dependencies (but isolated)
* ⚠️ **Git Size**: More code in one repo (mitigated by good .gitignore)

## 7. Implementation Details

### Desktop App Integration

**Cargo.toml** (`apps/dctx-desktop/src-tauri/Cargo.toml`):
```toml
[package]
name = "dctx-desktop"
license = "PROPRIETARY"

[dependencies]
dctx-core = { path = "../../../crates/dctx-core" }  # Direct dependency
tauri = "2"
```

**Rust Backend** (`apps/dctx-desktop/src-tauri/src/main.rs`):
```rust
use dctx_core::{walker, FilterConfig};

#[tauri::command]
fn generate_snapshot(path: String) -> Result<String, String> {
    // Direct call to dctx-core
    let tree = walker::walk_directory_structure(...)?;
    Ok(llm_formatter::format_snapshot(&tree))
}
```

**Frontend** (`apps/dctx-desktop/src/main.ts`):
```typescript
import { invoke } from "@tauri-apps/api/core";

const snapshot = await invoke("generate_snapshot", { path });
```

### Future Bindings Integration

**Python** (`bindings/dctx-py/Cargo.toml`):
```toml
[dependencies]
dctx-core = { path = "../../crates/dctx-core" }  # Same pattern
pyo3 = "0.20"
```

**Node.js** (`bindings/dctx-node/Cargo.toml`):
```toml
[dependencies]
dctx-core = { path = "../../crates/dctx-core" }  # Same pattern
napi = "2"
```

### CI/CD Updates

**GitHub Actions** (`.github/workflows/ci.yml`):
```yaml
- name: Test Rust workspace
  run: cargo test --workspace

- name: Build CLI
  run: cargo build --release --package dctx-cli

- name: Build Desktop (optional)
  run: cargo build --release --package dctx-desktop
```

### Version Synchronization

All components share the same version number:
- `VERSION` file: `0.11.0`
- `crates/dctx-core/Cargo.toml`: `version = "0.11.0"`
- `crates/dctx-cli/Cargo.toml`: `version = "0.11.0"`
- `apps/dctx-desktop/src-tauri/Cargo.toml`: `version = "0.11.0"`
- `apps/dctx-desktop/package.json`: `version = "0.11.0"`

Automated by `tools/sync-version.sh`.

## 8. Verification

### Test Workspace Builds

```bash
# Build entire Rust workspace
cargo build --workspace

# Test entire workspace
cargo test --workspace

# Build specific components
cargo build --package dctx-core
cargo build --package dctx-cli
cargo build --package dctx-desktop
```

### Verify Licensing

```bash
# MIT-licensed components
cat crates/dctx-core/Cargo.toml   # license = "MIT"
cat crates/dctx-cli/Cargo.toml    # license = "MIT"

# Proprietary components
cat apps/dctx-desktop/src-tauri/Cargo.toml  # license = "PROPRIETARY"
cat apps/dctx-desktop/LICENSE               # PROPRIETARY terms
```

## 9. Examples from Industry

**Successful Monorepos with Mixed Licensing:**

| Project | Structure | Licensing |
|---------|-----------|-----------|
| **GitLab** | `gitlab-ce/` (MIT) + `gitlab-ee/` (Proprietary) | Open Core |
| **Sentry** | `sentry/` (BSL) + `getsentry/` (Proprietary) | Open Core |
| **Tauri** | `tauri/` (MIT) + commercial support | Open Core |
| **Turborepo** | Monorepo with multiple packages | Mixed |
| **Next.js** | Monorepo with framework + tools | Mixed |

**Our model mirrors GitLab/Sentry**: Free core, paid GUI.

## 10. Future Considerations

**Phase 12 (Python Bindings)**:
- Add `bindings/dctx-py/` to workspace
- Publish to PyPI as `dctx`
- MIT licensed

**Phase 13 (Node.js Bindings)**:
- Add `bindings/dctx-node/` to workspace
- Publish to npm as `dctx`
- MIT licensed

**Phase 14+ (Other Bindings)**:
- Go, Ruby, etc. can be added to `bindings/` as needed
- All MIT licensed

## 11. Migration Checklist

- [x] Create `apps/` directory
- [x] Create `bindings/` directory
- [x] Create `docs/` directory
- [x] Initialize Tauri app in `apps/dctx-desktop/`
- [x] Add Tauri backend calling dctx-core
- [x] Add basic frontend (HTML + TypeScript)
- [x] Create proprietary LICENSE for desktop app
- [x] Update root `Cargo.toml` workspace members
- [x] Update root README with monorepo structure
- [x] Create bindings/README.md placeholder
- [x] Create apps/dctx-desktop/README.md
- [x] Add .gitignore for desktop app
- [x] Create this ADR

---

**Decision Date:** January 25, 2026
**Implemented By:** Monorepo Restructuring
**Status:** ✅ Accepted and Implemented
**Impact:** Foundational change enabling desktop app and future bindings
