// SPDX-License-Identifier: Apache-2.0
//! Legacy export dry-run. Inspection is a pure function of records the caller already holds.
//! Implements CACHE-v1 legacy safety: place, ownership, version and unchanged bytes are all required.
//! Depends on [`Lifecycle::legacy_dry_run`]. Apply removes only an adopted candidate that still matches.

use dctx_adapter_cache::{
    ExportEvidence, KnownExport, LeaveReason, Lifecycle, OutputPlace, Quota, RECOGNIZED_LEGACY_VERSION,
};

fn owned(place: OutputPlace, label: &str, identity: u64) -> KnownExport {
    KnownExport {
        place,
        label: label.to_string(),
        bytes: 12,
        evidence: ExportEvidence::OwnedBinary { version: RECOGNIZED_LEGACY_VERSION, identity },
    }
}

#[test]
fn legacy_dry_run_never_touches_unowned_exports() {
    let mut life = Lifecycle::new(Quota::scaled(100)).expect("quota");
    let registry = owned(OutputPlace::Registry, "registry/cache.bin", 9);
    let configured = KnownExport {
        place: OutputPlace::Configured,
        label: "out/notes.diff".to_string(),
        bytes: 4,
        evidence: ExportEvidence::OwnedDiff { identity: 4 },
    };
    let extension = KnownExport {
        place: OutputPlace::Registry,
        label: "notes.bin".to_string(),
        bytes: 2,
        evidence: ExportEvidence::ExtensionOnly,
    };
    let modified = KnownExport {
        place: OutputPlace::Configured,
        label: "out/changed.txt".to_string(),
        bytes: 2,
        evidence: ExportEvidence::Modified { identity: 5 },
    };
    let unknown = KnownExport {
        place: OutputPlace::Registry,
        label: "old.bin".to_string(),
        bytes: 2,
        evidence: ExportEvidence::UnknownVersion { version: 99 },
    };
    let unregistered = KnownExport {
        place: OutputPlace::Configured,
        label: "stray.bin".to_string(),
        bytes: 2,
        evidence: ExportEvidence::Unregistered,
    };
    let arbitrary = owned(OutputPlace::Arbitrary, "/tmp/cache.bin", 8);
    let exports = [
        registry.clone(),
        configured,
        extension.clone(),
        modified,
        unknown,
        unregistered,
        arbitrary,
    ];
    let revision = life.logical_revision();
    let touches = life.touches();
    let plan = life.legacy_dry_run(&exports);
    assert_eq!(life.logical_revision(), revision);
    assert_eq!(life.touches(), touches);
    assert_eq!(plan.candidates.len(), 2);
    assert!(plan.candidates.iter().any(|row| row.identity == 9));
    assert!(plan.candidates.iter().any(|row| row.identity == 4));
    assert!(plan.left_alone.iter().any(|row| row.reason == LeaveReason::ExtensionOnly));
    assert!(plan.left_alone.iter().any(|row| row.reason == LeaveReason::Modified));
    assert!(plan.left_alone.iter().any(|row| row.reason == LeaveReason::UnknownVersion));
    assert!(plan.left_alone.iter().any(|row| row.reason == LeaveReason::Unregistered));
    assert!(plan.left_alone.iter().any(|row| row.reason == LeaveReason::NotConfiguredPlace));

    assert!(life.adopt_legacy(&extension).is_err());
    assert_eq!(life.logical_revision(), revision);
    let refused = life.legacy_apply(&exports, &[9, 5, 8]).expect("apply");
    assert_eq!(refused.reclaimed, [] as [dctx_adapter_cache::ReclaimedExport; 0]);
    assert_eq!(life.touches(), 0);
    assert!(refused.untouched.iter().any(|row| row.reason == LeaveReason::NotAdopted));
    assert!(refused.untouched.iter().any(|row| row.reason == LeaveReason::Modified));
    assert!(refused.untouched.iter().any(|row| row.reason == LeaveReason::NotConfiguredPlace));

    life.adopt_legacy(&registry).expect("adopt");
    assert!(life.logical_revision() > revision);
    let applied = life.legacy_apply(&exports, &[9]).expect("apply");
    assert_eq!(applied.reclaimed.len(), 1);
    assert_eq!(applied.reclaimed[0].bytes, 12);
    assert_eq!(life.touches(), 1);
    assert_eq!(life.adopted_labels(), [] as [std::string::String; 0]);

    life.adopt_legacy(&registry).expect("adopt");
    let changed = KnownExport { evidence: ExportEvidence::Modified { identity: 9 }, ..registry };
    let stayed = life.legacy_apply(&[changed], &[9]).expect("apply");
    assert_eq!(stayed.reclaimed, [] as [dctx_adapter_cache::ReclaimedExport; 0]);
    assert_eq!(stayed.untouched[0].reason, LeaveReason::Modified);
    assert_eq!(life.touches(), 1);
    assert_eq!(life.adopted_labels(), vec!["registry/cache.bin".to_string()]);
}
