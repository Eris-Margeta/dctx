// SPDX-License-Identifier: Apache-2.0
//! `SHA256SUMS`: a generated convenience mirror that must agree with the signed manifest.
//! Implements PLUGINS.md §5.2 and distribution.md "SHA256SUMS mirrors each authenticated target
//! filename/hash; any disagreement refuses the release". It is never a trust source by itself.
//! Pure; depends on manifest and digest. Used by the install service when a release publishes one.

use std::collections::BTreeMap;

use super::digest::Sha256Hex;
use super::manifest::Manifest;
use crate::error::{DistributionError, Document};

/// Largest accepted mirror file.
pub const MAX_SUMS_BYTES: u64 = 65_536;

/// Refuses unless `sums` lists exactly the manifest's payloads with identical digests.
/// # Errors
/// Refuses oversized/non-UTF-8/malformed sums, duplicate names, or entries that differ from the signed manifest.
pub fn check_sums(manifest: &Manifest, sums: &[u8]) -> Result<(), DistributionError> {
    if sums.len() as u64 > MAX_SUMS_BYTES {
        return Err(DistributionError::LimitExceeded {
            limit: MAX_SUMS_BYTES,
        });
    }
    let text = std::str::from_utf8(sums)
        .map_err(|_| DistributionError::malformed(Document::Sums, "utf-8"))?;
    let mut listed = BTreeMap::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let (hex, name) = line
            .split_once("  ")
            .or_else(|| line.split_once(" *"))
            .ok_or(DistributionError::malformed(Document::Sums, "line"))?;
        let digest = Sha256Hex::parse(hex, Document::Sums)?;
        if listed.insert(name, digest).is_some() {
            return Err(DistributionError::malformed(
                Document::Sums,
                "duplicate-name",
            ));
        }
    }
    let mut signed = BTreeMap::new();
    for payload in manifest
        .artifacts
        .iter()
        .flat_map(super::manifest::Artifact::payloads)
    {
        signed.insert(payload.file.as_str(), payload.sha256);
    }
    if listed == signed {
        Ok(())
    } else {
        Err(DistributionError::SumsMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIT: &[u8] =
        include_bytes!("../../../../docs/design/plugin-api/fixtures/valid/manifest-git.json");
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn exact_mirror_is_accepted_and_any_disagreement_refused() {
        let manifest = Manifest::parse(GIT).unwrap();
        assert!(check_sums(&manifest, format!("{ABC}  abc.bin\n").as_bytes()).is_ok());
        assert!(check_sums(&manifest, format!("{ABC} *abc.bin\n").as_bytes()).is_ok());
        let other = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(
            check_sums(&manifest, format!("{other}  abc.bin\n").as_bytes()),
            Err(DistributionError::SumsMismatch)
        );
        assert_eq!(
            check_sums(
                &manifest,
                format!("{ABC}  abc.bin\n{other}  extra.bin\n").as_bytes()
            ),
            Err(DistributionError::SumsMismatch)
        );
        assert_eq!(
            check_sums(&manifest, b""),
            Err(DistributionError::SumsMismatch)
        );
        assert!(
            check_sums(
                &manifest,
                format!("{ABC}  abc.bin\n{ABC}  abc.bin\n").as_bytes()
            )
            .is_err()
        );
        assert!(check_sums(&manifest, b"garbage\n").is_err());
    }
}
