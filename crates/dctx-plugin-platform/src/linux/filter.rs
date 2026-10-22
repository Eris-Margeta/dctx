//! Deny-default syscall policy for already-loaded, single-threaded Rust computation.
//! PL-R Linux profile: no opens, exec, clone, detach, network, tracing or namespace APIs.
//! Syscall numbers are the exact x86-64/AArch64 Linux tables reviewed in seccompiler 0.5.0.

use crate::{PlatformError, Unsupported};
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule, TargetArch,
};
use std::collections::BTreeMap;

fn failure() -> PlatformError {
    PlatformError::Unsupported(Unsupported::Seccomp)
}

fn condition(index: u8, op: SeccompCmpOp, value: u64) -> Result<SeccompCondition, PlatformError> {
    SeccompCondition::new(index, SeccompCmpArgLen::Qword, op, value).map_err(|_| failure())
}

fn rule(conditions: Vec<SeccompCondition>) -> Result<SeccompRule, PlatformError> {
    SeccompRule::new(conditions).map_err(|_| failure())
}

pub(super) fn compile() -> Result<BpfProgram, PlatformError> {
    let (arch, plain, calls): (TargetArch, &[i64], [i64; 7]) = if cfg!(target_arch = "x86_64") {
        (
            TargetArch::x86_64,
            &[
                11, 12, 13, 14, 15, 24, 28, 39, 60, 131, 186, 202, 219, 228, 231, 318,
            ],
            [0, 1, 20, 3, 5, 9, 10],
        )
    } else if cfg!(target_arch = "aarch64") {
        (
            TargetArch::aarch64,
            &[
                93, 94, 98, 113, 124, 128, 132, 134, 135, 139, 172, 178, 214, 215, 233, 278,
            ],
            [63, 64, 66, 57, 80, 222, 226],
        )
    } else {
        return Err(failure());
    };
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = plain.iter().map(|&id| (id, vec![])).collect();
    let [read, write, writev, close, fstat, mmap, mprotect] = calls;
    rules.insert(read, vec![rule(vec![condition(0, SeccompCmpOp::Eq, 0)?])?]);
    for id in [write, writev] {
        rules.insert(
            id,
            vec![
                rule(vec![condition(0, SeccompCmpOp::Eq, 1)?])?,
                rule(vec![condition(0, SeccompCmpOp::Eq, 2)?])?,
            ],
        );
    }
    for id in [close, fstat] {
        rules.insert(id, vec![rule(vec![condition(0, SeccompCmpOp::Le, 2)?])?]);
    }
    // Only private anonymous non-executable allocation; no file-backed mappings or JIT.
    rules.insert(
        mmap,
        vec![rule(vec![
            condition(2, SeccompCmpOp::MaskedEq(4), 0)?,
            condition(3, SeccompCmpOp::MaskedEq(0x23), 0x22)?,
        ])?],
    );
    rules.insert(
        mprotect,
        vec![rule(vec![condition(2, SeccompCmpOp::MaskedEq(4), 0)?])?],
    );
    SeccompFilter::new(
        rules,
        SeccompAction::KillProcess,
        SeccompAction::Allow,
        arch,
    )
    .map_err(|_| failure())?
    .try_into()
    .map_err(|_| failure())
}

pub(super) fn install(filter: &BpfProgram) -> Result<(), PlatformError> {
    seccompiler::apply_filter_all_threads(filter).map_err(|_| failure())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_compiles_within_kernel_instruction_limit() {
        if cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) {
            let program = compile().unwrap();
            assert_ne!(program, []);
            assert!(program.len() < 4096);
        } else {
            assert!(compile().is_err());
        }
    }
}
