//! Atomic file-owned replacement of graph facts and lexical rows.
//! Implements map spec section 7 without deleting foreign-owned edges.
//! Uses SQLite transactions; called by the persistence adapter.
use crate::domain::*;
use rusqlite::{Transaction, params};
use std::collections::BTreeSet;

pub(super) fn validate(facts: &FileFacts) -> Result<()> {
    let mut ids = BTreeSet::new();
    for node in &facts.nodes {
        let identity_path =
            node.id
                .as_str()
                .split_once(':')
                .map(|(_, rest)| match rest.split_once('#') {
                    Some((path, _)) => path,
                    None => rest,
                });
        if node.path != facts.file.path
            || identity_path != Some(node.path.as_str())
            || !node.id.as_str().starts_with(node.kind.letter())
            || node.lines[0] == 0
            || node.lines[1] < node.lines[0]
            || !ids.insert(&node.id)
        {
            return Err(StoreError::InvalidFacts("node ownership, identity or span"));
        }
    }
    if facts
        .edges
        .iter()
        .any(|edge| edge.owner_path != facts.file.path || !edge.kind.permits(edge.conf))
    {
        return Err(StoreError::InvalidFacts("edge ownership or confidence"));
    }
    for summary in &facts.summaries {
        if !facts
            .nodes
            .iter()
            .any(|node| node.id == summary.id && node.fingerprint == summary.fingerprint)
            || summary.text.is_empty()
            || summary.text.contains(['\n', '\r'])
            || summary.text.chars().count() > 100
        {
            return Err(StoreError::InvalidFacts(
                "summary identity, fingerprint or text",
            ));
        }
    }
    Ok(())
}

pub(super) fn remove(tx: &Transaction<'_>, path: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM fts WHERE id IN (SELECT id FROM nodes WHERE path=?1)",
        [path],
    )?;
    tx.execute(
        "DELETE FROM summaries WHERE id IN (SELECT id FROM nodes WHERE path=?1)",
        [path],
    )?;
    tx.execute("DELETE FROM nodes WHERE path=?1", [path])?;
    tx.execute("DELETE FROM edges WHERE owner_path=?1", [path])?;
    tx.execute("DELETE FROM unresolved WHERE owner_path=?1", [path])?;
    tx.execute("DELETE FROM files WHERE path=?1", [path])?;
    Ok(())
}

pub(super) fn insert(tx: &Transaction<'_>, facts: &FileFacts) -> Result<()> {
    let file = &facts.file;
    tx.execute(
        "INSERT INTO files VALUES(?1,?2,?3,?4,?5)",
        params![file.path, file.size, file.mtime, file.sha256, file.lang],
    )?;
    for node in &facts.nodes {
        tx.prepare_cached("INSERT INTO nodes VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)")?
            .execute(params![
                node.id.as_str(),
                node.kind.as_str(),
                node.path,
                node.lines[0],
                node.lines[1],
                node.name,
                node.signature,
                node.fingerprint,
                node.public,
                serde_json::to_string(node)?
            ])?;
        let mut text = format!(
            "{} {} {} {}",
            super::tokens::tokens(&node.name),
            super::tokens::tokens(&node.path),
            node.summary.text,
            node.signature
        );
        for summary in facts
            .summaries
            .iter()
            .filter(|summary| summary.id == node.id)
        {
            text.push(' ');
            text.push_str(&summary.text);
        }
        tx.prepare_cached("INSERT INTO fts(id,tokens) VALUES(?1,?2)")?
            .execute(params![node.id.as_str(), text])?;
    }
    for edge in &facts.edges {
        tx.prepare_cached("INSERT INTO edges VALUES(?1,?2,?3,?4,?5,?6)")?
            .execute(params![
                edge.src.as_str(),
                edge.dst.as_str(),
                edge.kind.as_str(),
                edge.conf.as_str(),
                edge.owner_path,
                serde_json::to_string(edge)?
            ])?;
    }
    for reference in &facts.unresolved {
        tx.prepare_cached("INSERT INTO unresolved VALUES(?1,?2,?3)")?
            .execute(params![file.path, reference.name, reference.kind.as_str()])?;
    }
    for summary in &facts.summaries {
        tx.prepare_cached("INSERT INTO summaries VALUES(?1,?2,?3,'llm',?4,?5)")?
            .execute(params![
                summary.id.as_str(),
                summary.fingerprint,
                summary.text,
                summary.model,
                summary.at
            ])?;
    }
    Ok(())
}
