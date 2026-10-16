# ADR-013: Monorepo Structure

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
- Components, which are all Apache-2.0
- The desktop app, sold as signed builds with updates (a way to support the project)
- Future language bindings
- Distribution artifacts

## 2. Decision Drivers

* **Clear Licensing**: Everything is Apache-2.0
* **Monorepo Benefits**: Single source of truth, shared CI/CD, atomic commits
* **Scalability**: Easy to add bindings without restructuring again
* **Business Model**: The desktop app is sold as signed builds with updates (a way to support the project)
* **Developer Experience**: Obvious where each component lives
* **Distribution**: Multiple distribution formats from one repo
* **Code Reuse**: Desktop app and bindings use same dctx-core

## 3. Considered Options

### Option 1: Multi-Repo (Rejected)

**Structure:**
```
github.com/dctx/dctx-core       (Apache-2.0)
github.com/dctx/dctx-cli        (Apache-2.0)
github.com/dctx/dctx-desktop    (Apache-2.0; sold as signed builds with updates)
github.com/dctx/dctx-py         (Apache-2.0)
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
├── crates/        # Rust workspace (Apache-2.0)
├── apps/          # Applications (Apache-2.0)
├── bindings/      # Language bindings (Apache-2.0)
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
├── crates/                    # Apache-2.0 Rust Workspace
│   ├── dctx-core/             # Core library (0 external deps)
│   └── dctx-cli/              # CLI binary
│
├── apps/                      # Applications
│   └── dctx-desktop/          # Apache-2.0 - Tauri GUI, sold as signed builds with updates
│       ├── src/               # Frontend (TypeScript)
│       ├── src-tauri/         # Backend (Rust + dctx-core)
│       ├── LICENSE            # Apache-2.0
│       └── README.md
│
├── bindings/                  # Apache-2.0 Language Bindings
│   ├── dctx-py/               # Python (PyO3) - Phase 12
│   └── dctx-node/             # Node.js (NAPI-RS) - Phase 13
│
├── tools/                     # Build and development scripts
├── docs/                      # Documentation
├── website/                   # Landing page
├── .github/                   # CI/CD workflows
│
├── Cargo.toml                 # Workspace definition
├── LICENSE                    # Apache-2.0 (applies to the whole repository)
└── README.md                  # Explains licensing model
```

### Licensing Model

**Everything is Apache-2.0** (decision 078, 16.10.2026). Copyright holder: Eris Margeta.
- `crates/dctx-core/` - Core library
- `crates/dctx-cli/` - CLI with ALL features
- `bindings/dctx-py/` - Python bindings (future)
- `bindings/dctx-node/` - Node.js bindings (future)
- `apps/dctx-desktop/` - Desktop GUI application
- Root `LICENSE` applies to the whole repository
- `apps/dctx-desktop/LICENSE` is the same Apache-2.0 text
- The desktop app is sold as signed builds with updates (a way to support the project)

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
5. ✅ Add Apache-2.0 LICENSE to desktop app
6. ✅ Update root `Cargo.toml` workspace members
7. ✅ Update root README explaining structure
8. ✅ Create placeholder READMEs in `bindings/`

## 5. Positive Consequences

* ✅ **Clear Licensing**: Everything is Apache-2.0
* ✅ **Atomic Commits**: Cross-component changes in single commit
* ✅ **Shared CI/CD**: One workflow tests everything
* ✅ **Code Reuse**: Desktop app uses dctx-core directly (no duplication)
* ✅ **Scalability**: Easy to add Python/Node.js bindings
* ✅ **Single Version**: All components share version number (0.11.0)
* ✅ **Developer Experience**: Clear structure, obvious where things go
* ✅ **Business Model**: The desktop app is sold as signed builds with updates (a way to support the project)
* ✅ **Transparency**: Desktop app source is Apache-2.0 and visible

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
license = "Apache-2.0"

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
# Apache-2.0 components
cat crates/dctx-core/Cargo.toml   # license = "Apache-2.0"
cat crates/dctx-cli/Cargo.toml    # license = "Apache-2.0"

# Desktop app (Apache-2.0; sold as signed builds with updates)
cat apps/dctx-desktop/src-tauri/Cargo.toml  # license = "Apache-2.0"
cat apps/dctx-desktop/LICENSE               # Apache-2.0 terms
```

## 9. Examples from Industry

**Monorepos that ship more than one distribution from one tree:**

| Project | Structure | Licensing |
|---------|-----------|-----------|
| **GitLab** | `gitlab-ce/` + `gitlab-ee/` | Open core |
| **Sentry** | `sentry/` (BSL) + `getsentry/` | Open core |
| **Tauri** | `tauri/` + commercial support | Open core |
| **Turborepo** | Monorepo with multiple packages | Several packages |
| **Next.js** | Monorepo with framework + tools | Several packages |

**dctx**: Everything is Apache-2.0. The desktop app is sold as signed builds with updates (a way to support the project).

## 10. Future Considerations

**Phase 12 (Python Bindings)**:
- Add `bindings/dctx-py/` to workspace
- Publish to PyPI as `dctx`
- Apache-2.0

**Phase 13 (Node.js Bindings)**:
- Add `bindings/dctx-node/` to workspace
- Publish to npm as `dctx`
- Apache-2.0

**Phase 14+ (Other Bindings)**:
- Go, Ruby, etc. can be added to `bindings/` as needed
- All Apache-2.0

## 11. Migration Checklist

- [x] Create `apps/` directory
- [x] Create `bindings/` directory
- [x] Create `docs/` directory
- [x] Initialize Tauri app in `apps/dctx-desktop/`
- [x] Add Tauri backend calling dctx-core
- [x] Add basic frontend (HTML + TypeScript)
- [x] Create Apache-2.0 LICENSE for desktop app
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
