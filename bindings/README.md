# dctx Language Bindings

This directory contains language bindings for the `dctx-core` Rust library.

**Status**: Planned for post-v1.0

---

## Planned Bindings

### Python (Phase 12)
**Directory**: `bindings/dctx-py/`
**Technology**: PyO3
**License**: MIT
**Target**: PyPI (`pip install dctx`)

**Use Cases**:
- Jupyter notebooks analyzing codebases
- AI/ML automation pipelines
- Build scripts and CI/CD
- Python-based tools

**Example**:
```python
import dctx

snapshot = dctx.snapshot(
    path=".",
    filters={"ext": ["py", "md"], "max_size_mb": 1.0}
)
context = snapshot.to_llm_format()
```

---

### Node.js (Phase 13)
**Directory**: `bindings/dctx-node/`
**Technology**: NAPI-RS
**License**: MIT
**Target**: npm (`npm install dctx`)

**Use Cases**:
- VS Code extensions
- Build tools (Webpack, Vite, Rollup plugins)
- GitHub Actions
- Electron apps

**Example**:
```javascript
const dctx = require('dctx');

const snapshot = await dctx.snapshot({
    path: '.',
    filters: { ext: ['ts', 'js'] }
});
const context = snapshot.toLlmFormat();
```

---

### Go (Future - If Demand Exists)
**Directory**: `bindings/dctx-go/`
**Technology**: cgo or FFI
**License**: MIT

---

## Architecture

All bindings wrap the core Rust library (`dctx-core`) without duplicating logic:

```
┌─────────────────────────────────────┐
│   Language-Specific API             │
│   (Python, Node.js, Go)             │
├─────────────────────────────────────┤
│   FFI/Binding Layer                 │
│   (PyO3, NAPI-RS, cgo)              │
├─────────────────────────────────────┤
│   dctx-core (Rust)                  │
│   (Single source of truth)          │
└─────────────────────────────────────┘
```

**Benefits**:
- Zero code duplication
- Single source of truth (dctx-core)
- Native performance
- Type safety

---

## Current Status

**Available Now**: Rust library (`dctx-core`)
**Planned Q2 2026**: Python bindings
**Planned Q3 2026**: Node.js bindings (if demand)

---

For Rust usage, see the main [README](../README.md#use-as-library).
