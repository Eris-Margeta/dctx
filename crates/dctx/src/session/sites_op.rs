//! Call-site lines for a neighbors page. Reads each caller's file once through the same verified read as `read`.
//! Implements the `sites` field of inbound `calls` records. A file that cannot be read or verified leaves its records without sites.
//! Depends on [`crate::index::sites`] for the plan and the scan.

use std::collections::HashMap;

use super::Session;
use crate::domain::{AnswerRecord, MapRequest};
use crate::index::{sites, Generation};

impl Session {
    /// Fills `sites` on the depth-one inbound `calls` records of `records`. Other records stay unchanged.
    pub(super) fn attach_sites(&mut self, generation: &Generation, request: &MapRequest, records: &mut [AnswerRecord]) {
        let Some(plan) = sites::plan(generation, request, records) else {
            return;
        };
        let mut files: HashMap<String, Option<Vec<u8>>> = HashMap::new();
        for job in &plan.jobs {
            if !files.contains_key(&job.path) {
                let digest = generation.files.iter().find(|file| file.path == job.path).and_then(|file| file.digest);
                let bytes = match digest {
                    Some(expected) if self.authorize(&job.path).is_ok() => self.read_verified(&job.path, expected).ok(),
                    _ => None,
                };
                files.insert(job.path.clone(), bytes);
            }
            if let Some(Some(bytes)) = files.get(&job.path)
                && let Some(record) = records.get_mut(job.index)
            {
                record.sites = sites::scan(bytes, job, &plan.name);
            }
        }
    }
}
