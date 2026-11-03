// SPDX-License-Identifier: Apache-2.0
//! Unit coverage for unresolved receipt evidence in PLUGINS.md §4.4.
//! Uses in-memory durable frames to check intent/completion replay.
//! Runs with the library and integration tests that include the audit adapter.

use super::*;

#[derive(Clone, Default)]
struct Memory(Vec<u8>);

impl AuditStorage for Memory {
    fn read(&mut self, _: usize) -> Result<Vec<u8>, AuditUnavailable> {
        Ok(self.0.clone())
    }

    fn append_sync(&mut self, frame: &[u8]) -> Result<(), AuditUnavailable> {
        self.0.extend_from_slice(frame);
        Ok(())
    }
}

#[test]
fn pending_receipt_survives_reopen_and_completion_clears_it() {
    let mut audit = DurableAudit::new(Memory::default(), 8192).unwrap();
    let mut record = AuditRecord {
        kind: AuditKind::Intent,
        plugin: PluginId::new("dctx/test").unwrap(),
        invocation: Token::new("op:1").unwrap(),
        epoch: 1,
        method: "file.replace",
        handle: None,
        args: None,
        bytes: 0,
        receipt: Some(Token::new("receipt:1").unwrap()),
        reason: "intent",
    };
    audit.append(&record).unwrap();
    let pending = vec![("dctx/test".to_owned(), "op:1".to_owned(), "receipt:1".to_owned())];
    assert_eq!(audit.uncertain(), pending);

    let mut reopened = DurableAudit::new(audit.storage.clone(), 8192).unwrap();
    assert_eq!(reopened.uncertain(), pending);
    record.kind = AuditKind::Completion;
    record.reason = "committed";
    reopened.append(&record).unwrap();
    assert_eq!(reopened.uncertain(), [] as [(std::string::String, std::string::String, std::string::String); 0]);
    assert_eq!(DurableAudit::new(reopened.storage, 8192).unwrap().uncertain(), [] as [(std::string::String, std::string::String, std::string::String); 0]);
}
