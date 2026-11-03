// SPDX-License-Identifier: Apache-2.0
//! Exact, privacy-checked source windows for the read verb.
//! Implements SPEC-MAP-MODE sections 4 and 7 with hash validation.
//! Reads through the Source port and refuses stale spans or omitted content.
use super::Index;
use crate::{Answer, Error, Record, Request, Result, ports::Source};
use dctx_map_model::StableId;

impl Index {
    pub(super) fn read_record(
        &self,
        source: &impl Source,
        request: &Request,
        id: &StableId,
        answer: &mut Answer,
    ) -> Result<Vec<Record>> {
        let node = &self.nodes[id];
        let Some(file) = self.files.get(&node.path) else {
            answer.reason = Some("this structural node has no source span".into());
            return Ok(vec![]);
        };
        if !file.readable {
            answer.reason = Some("source omitted by scan policy".into());
            answer.identity = None;
            return Ok(vec![]);
        }
        if self.report.stale.contains_key(&node.path) {
            answer.reason = Some(format!("stale: {} could not be refreshed", node.path));
            answer.identity = None;
            return Ok(vec![]);
        }
        let bytes = source.read(&node.path)?;
        if super::source::digest(&bytes)? != file.hash {
            answer.reason = Some("stale: source changed after indexing".into());
            answer.identity = None;
            return Ok(vec![]);
        }
        let text = std::str::from_utf8(&bytes).map_err(|e| Error::Request(e.to_string()))?;
        let maximum = if request.full { 400 } else { 120 };
        let [start, end] = request.lines.unwrap_or([
            node.lines[0],
            node.lines[1].min(node.lines[0].saturating_add(399)),
        ]);
        if start < node.lines[0] || end > node.lines[1] {
            return Err(Error::Request(
                "--lines must stay within the node span".into(),
            ));
        }
        let lines: Vec<_> = text.split_inclusive('\n').collect();
        let mut record = Record::from(node);
        record.l = [start, end];
        record.body = Some(
            lines
                .iter()
                .skip(start as usize - 1)
                .take((end - start + 1) as usize)
                .copied()
                .collect(),
        );
        if end < node.lines[1] {
            let mut next = request.clone();
            next.lines = Some([end + 1, node.lines[1].min(end.saturating_add(maximum))]);
            answer.more = 1;
            answer.next = next.command(0);
        }
        Ok(vec![record])
    }
}
