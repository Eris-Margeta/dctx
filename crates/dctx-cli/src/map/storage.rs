//! Retired per-worktree sqlite cache selector.
//! Map mode now calls the facade ephemeral session (ARCHITECTURE-PROPOSAL §4 and §7 DX-D2).
//! This file is not a module. The CLI must not create `~/.cache/dctx/map` for a query.
//! `DCTX_MAP_CACHE` remains a documented compatibility name and is not read here.
