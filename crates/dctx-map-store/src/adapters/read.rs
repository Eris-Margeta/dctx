// SPDX-License-Identifier: Apache-2.0
//! Reconstructs typed model facts from deterministic SQLite rows.
//! Implements the file read boundary for map spec section 8.
//! Used by SQLite callers and replacement-invariant tests.
use crate::domain::{File, FileFacts, Result, StoredSummary, Unresolved};
use rusqlite::Connection;

pub(super) fn facts(connection: &Connection, file: File) -> Result<FileFacts> {
    let mut nodes =
        connection.prepare_cached("SELECT payload FROM nodes WHERE path=?1 ORDER BY id")?;
    let nodes = nodes
        .query_map([&file.path], |row| row.get::<_, String>(0))?
        .map(|row| Ok(serde_json::from_str(&row?)?))
        .collect::<Result<Vec<_>>>()?;
    let mut edges = connection.prepare_cached(
        "SELECT payload FROM edges WHERE owner_path=?1 ORDER BY src,dst,kind,conf",
    )?;
    let edges = edges
        .query_map([&file.path], |row| row.get::<_, String>(0))?
        .map(|row| Ok(serde_json::from_str(&row?)?))
        .collect::<Result<Vec<_>>>()?;
    let mut refs = connection.prepare_cached(
        "SELECT name,kind FROM unresolved WHERE owner_path=?1 ORDER BY name,kind",
    )?;
    let unresolved = refs
        .query_map([&file.path], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .map(|row| {
            let (name, kind) = row?;
            Ok(Unresolved {
                name,
                kind: serde_json::from_value(serde_json::Value::String(kind))?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut summaries = connection.prepare_cached(
        "SELECT s.id,s.fp,s.text,s.model,s.at FROM summaries s JOIN nodes n ON n.id=s.id
         WHERE n.path=?1 AND n.fp=s.fp ORDER BY s.id",
    )?;
    let summaries = summaries
        .query_map([&file.path], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .map(|row| {
            let (id, fingerprint, text, model, at) = row?;
            Ok(StoredSummary {
                id: id.parse()?,
                fingerprint,
                text,
                model,
                at,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(FileFacts {
        file,
        nodes,
        edges,
        unresolved,
        summaries,
    })
}
