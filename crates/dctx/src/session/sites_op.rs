//! Call-site lines for a neighbors page. Reads each caller's file once through the same verified read as `read`.
//! Implements the `sites` field of inbound `calls` records and the caller order: direct callers with more call sites come first.
//! A file that cannot be read or verified leaves its records without sites, ranked as one site.
//! Depends on [`crate::index::sites`] for the plan, the scan and the ranking.

use std::collections::HashMap;

use super::Session;
use crate::domain::{AnswerRecord, MapRequest};
use crate::index::{sites, Generation};

/// Most depth-one callers whose files are scanned to rank a whole answer. A larger answer keeps the target order and gets sites per page.
const RANK_CAP: usize = 2000;

impl Session {
    /// Scans every depth-one inbound `calls` record of the whole answer, fills its `sites` and ranks the callers with more call sites first.
    /// Returns false, leaving `records` untouched, when there is nothing to scan or more than [`RANK_CAP`] callers.
    pub(super) fn rank_sites(&mut self, generation: &Generation, request: &MapRequest, records: &mut Vec<AnswerRecord>) -> bool {
        let Some(plan) = sites::plan(generation, request, records) else {
            return false;
        };
        if plan.jobs.len() > RANK_CAP {
            return false;
        }
        let counts = self.fill_sites(generation, &plan, records);
        sites::rank(records, &counts);
        true
    }

    /// Fills `sites` on the depth-one inbound `calls` records of `records`. Other records stay unchanged.
    pub(super) fn attach_sites(&mut self, generation: &Generation, request: &MapRequest, records: &mut [AnswerRecord]) {
        if let Some(plan) = sites::plan(generation, request, records) {
            self.fill_sites(generation, &plan, records);
        }
    }

    /// Scans the jobs of `plan`, keeps the first [`sites::MAX_SITES`] lines on each record and returns every record's full call-site count, at least one for a direct caller.
    fn fill_sites(&mut self, generation: &Generation, plan: &sites::SitePlan, records: &mut [AnswerRecord]) -> Vec<usize> {
        let mut counts = vec![0; records.len()];
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
            // Every direct caller counts as at least one site, also one whose file cannot be read or whose call the scan does not see.
            if let Some(count) = counts.get_mut(job.index) {
                *count = 1;
            }
            if let Some(Some(bytes)) = files.get(&job.path)
                && let Some(record) = records.get_mut(job.index)
            {
                let mut lines = sites::scan(bytes, job, &plan.name);
                if let Some(count) = counts.get_mut(job.index) {
                    *count = lines.len().max(1);
                }
                lines.truncate(sites::MAX_SITES);
                record.sites = lines;
            }
        }
        counts
    }
}

/// Fits a neighbors page into the budget, where `fits(records, more)` says whether that page fits.
/// Summaries are shed before records: every summary goes, then records drop from the end (never the last one, so the caller's
/// refusal keeps its size), then summaries come back from the top while the page still fits. Each dropped record adds one to `more`.
pub(super) fn fit_neighbors(records: &mut Vec<AnswerRecord>, more: &mut u64, fits: impl Fn(&[AnswerRecord], u64) -> bool) {
    if records.is_empty() || fits(records, *more) {
        return;
    }
    let summaries: Vec<String> = records.iter_mut().map(|record| std::mem::take(&mut record.summary)).collect();
    while records.len() > 1 && !fits(records, *more) {
        records.pop();
        *more = more.saturating_add(1);
    }
    for (index, summary) in summaries.into_iter().enumerate().take(records.len()) {
        if summary.is_empty() {
            continue;
        }
        records[index].summary = summary;
        if !fits(records, *more) {
            records[index].summary.clear();
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `count` records with a 100-byte summary each.
    fn page(count: usize) -> Vec<AnswerRecord> {
        (0..count)
            .map(|index| {
                let mut record = AnswerRecord::basic(format!("calls:in:s:a.rs#f{index}"), "fn", "a.rs");
                record.summary = "x".repeat(100);
                record
            })
            .collect()
    }

    /// Bytes of a page: 50 per record plus its summary, plus 10 when more records follow.
    fn size(records: &[AnswerRecord], more: u64) -> usize {
        records.iter().map(|record| 50 + record.summary.len()).sum::<usize>() + if more > 0 { 10 } else { 0 }
    }

    #[test]
    fn summaries_go_before_records_and_come_back_from_the_top() {
        let mut records = page(10);
        let mut more = 0;
        fit_neighbors(&mut records, &mut more, |records, more| size(records, more) <= 650);
        // Ten bare records need 500 + 10 bytes; one summary fits back in, a second does not.
        assert_eq!((records.len(), more), (10, 0));
        assert_eq!(records.iter().filter(|record| !record.summary.is_empty()).count(), 1);
        assert!(!records[0].summary.is_empty());
        let mut records = page(10);
        let mut more = 0;
        fit_neighbors(&mut records, &mut more, |records, more| size(records, more) <= 300);
        assert_eq!((records.len(), more), (5, 5));
        assert!(records.iter().all(|record| record.summary.is_empty()));
    }

    #[test]
    fn a_page_that_fits_is_left_alone_and_the_last_record_is_kept() {
        let mut records = page(3);
        let mut more = 0;
        fit_neighbors(&mut records, &mut more, |records, more| size(records, more) <= 450);
        assert_eq!(records, page(3));
        let mut records = page(3);
        fit_neighbors(&mut records, &mut more, |_, _| false);
        assert_eq!((records.len(), more), (1, 2));
    }
}
