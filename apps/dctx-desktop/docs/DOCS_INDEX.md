# dctx Desktop - Documentation Index

Quick reference to all documentation files.

---

## Core Documents

### 📘 [README.md](README.md)
**Purpose**: Main user-facing documentation
**Contents**:
- Feature overview
- Quick start guide
- Installation instructions
- Usage examples
- Configuration basics
- Troubleshooting
- System requirements
- Licensing information

**Read this if**: You're a new user or want to understand what the app does.

---

### 🛠️ [DEVELOPMENT.md](DEVELOPMENT.md)
**Purpose**: Complete developer guide
**Contents**:
- Development setup
- Architecture overview
- Workflow and best practices
- Key concepts (commands, permissions, config)
- Testing guide
- Building for production
- Debugging tips
- Common tasks (adding commands, permissions, dependencies)
- Code style guide
- Performance optimization tips

**Read this if**: You're developing features or fixing bugs.

---

### ⚙️ [CONFIG.md](CONFIG.md)
**Purpose**: Configuration system documentation
**Contents**:
- Two-tier config hierarchy
- Global vs desktop config
- Configuration file locations
- All available settings
- Config inheritance behavior
- Platform-specific paths
- Future settings UI plans

**Read this if**: You want to configure the app or understand the config system.

---

### 🐛 [DRAG_DROP_DEBUG.md](DRAG_DROP_DEBUG.md)
**Purpose**: Drag & drop troubleshooting guide
**Contents**:
- What was fixed and how
- Step-by-step testing instructions
- Console debugging
- Common issues and solutions
- Expected behavior
- Manual test commands
- Full diagnostic checklist

**Read this if**: Drag & drop isn't working or you're debugging event handlers.

---

### 📝 [CHANGELOG.md](CHANGELOG.md)
**Purpose**: Version history and release notes
**Contents**:
- v0.1.0-alpha release details
- All features added
- All bugs fixed
- Technical changes
- Roadmap for v0.2.0, v0.3.0, v1.0.0
- Version history table

**Read this if**: You want to know what's new or what's coming next.

---

### ✅ [PHASE_11_COMPLETE.md](PHASE_11_COMPLETE.md)
**Purpose**: Phase 11 completion report
**Contents**:
- Complete feature summary
- Architecture decisions
- Problems solved (with root causes and solutions)
- Performance metrics
- Testing results
- Documentation delivered
- Next phase planning
- Lessons learned
- Commands reference

**Read this if**: You want a comprehensive overview of what was built and why.

---

## Metadata Files

### 📄 VERSION
```
0.1.0-alpha
```

Current version number.

---

### 📄 LICENSE
Apache-2.0 open source license. Free for any use, including commercial use. Paid support is available through a maintenance agreement; see MAINTENANCE.md in the repository root.

---

## Quick Navigation

### I want to...

| Goal | Read This |
|------|-----------|
| **Understand what the app does** | [README.md](README.md) |
| **Start using the app** | [README.md](README.md) → Quick Start |
| **Set up development environment** | [DEVELOPMENT.md](DEVELOPMENT.md) → Setup |
| **Fix a bug** | [DEVELOPMENT.md](DEVELOPMENT.md) → Debugging |
| **Add a new feature** | [DEVELOPMENT.md](DEVELOPMENT.md) → Common Tasks |
| **Configure settings** | [CONFIG.md](CONFIG.md) |
| **Debug drag & drop** | [DRAG_DROP_DEBUG.md](DRAG_DROP_DEBUG.md) |
| **See what changed** | [CHANGELOG.md](CHANGELOG.md) |
| **Understand architecture** | [PHASE_11_COMPLETE.md](PHASE_11_COMPLETE.md) → Architecture |
| **Check performance** | [PHASE_11_COMPLETE.md](PHASE_11_COMPLETE.md) → Metrics |
| **Plan next features** | [CHANGELOG.md](CHANGELOG.md) → Roadmap |

---

## Documentation Structure

```
apps/dctx-desktop/
├── README.md                 # Main user docs (13 KB)
├── DEVELOPMENT.md            # Developer guide (22 KB)
├── CONFIG.md                 # Configuration guide (3.5 KB)
├── DRAG_DROP_DEBUG.md        # Drag & drop debugging (8.5 KB)
├── CHANGELOG.md              # Version history (3 KB)
├── PHASE_11_COMPLETE.md      # Phase completion report (15 KB)
├── DOCS_INDEX.md             # This file
├── VERSION                   # Current version
└── LICENSE                   # Software license
```

**Total Documentation**: ~65 KB

---

## External Resources

### Official Documentation
- **Tauri**: https://tauri.app/v2/
- **Bun**: https://bun.sh/docs
- **Vite**: https://vitejs.dev/
- **TypeScript**: https://www.typescriptlang.org/docs/

### Related Projects
- **dctx-core** (Apache-2.0): `../../crates/dctx-core/`
- **dctx CLI** (Apache-2.0): `../../crates/dctx-cli/`

---

## Keeping Docs Updated

### When adding features:
1. Update **README.md** with user-facing changes
2. Update **CHANGELOG.md** with detailed changes
3. Update **DEVELOPMENT.md** if architecture changes
4. Create specific guides if complex (like DRAG_DROP_DEBUG.md)

### Before each release:
1. Update **VERSION** file
2. Update **CHANGELOG.md** with release date
3. Review **README.md** for accuracy
4. Update roadmap sections

---

**Last Updated**: January 26, 2026
**Version**: 0.1.0-alpha
