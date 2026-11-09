// SPDX-License-Identifier: Apache-2.0
//! Byte reports for cache status, gc and purge.
//! Implements ARCHITECTURE-PROPOSAL §4 status fields and reclaimed-byte reports.
//! Depends on adapter status types and the facade [`dctx::CacheReport`]. The command module prints this text.
//! JSON is hand-built so the CLI does not add a serializer.

use dctx::CacheReport;
use dctx_adapter_cache::{Blocker, MaintainReport, PurgeReport, Status, DAY_MS};
use std::fmt::Write as _;

use crate::host::{AutoReport, CachePolicy};

pub fn status(report: &Status, session: &CacheReport, json: bool) -> String {
    if json {
        return format!(
            "{{\"command\":\"status\",\"hard_cap\":{},\"soft_watermark\":{},\"eviction_target\":{},\"committed\":{},\"staged\":{},\"reserved\":{},\"pending\":{},\"admitted\":{},\"live_bytes\":{},\"reclaimable_bytes\":{},\"pinned_bytes\":{},\"authorization_revoked_bytes\":{},\"deletion_requested_bytes\":{},\"active_leases\":{},\"roots\":{},{}}}",
            report.hard_cap,
            report.soft_watermark,
            report.eviction_target,
            report.buckets.committed,
            report.buckets.staged,
            report.buckets.reserved,
            report.buckets.pending,
            report.admitted,
            report.live_bytes,
            report.reclaimable_bytes,
            report.pinned_bytes,
            report.authorization_revoked_bytes,
            report.deletion_requested_bytes,
            report.active_leases,
            report.roots.len(),
            session_json(session)
        );
    }
    format!(
        "\
command: status
hard_cap: {}
soft_watermark: {}
eviction_target: {}
committed: {}
staged: {}
reserved: {}
pending: {}
admitted: {}
live_bytes: {}
reclaimable_bytes: {}
pinned_bytes: {}
authorization_revoked_bytes: {}
deletion_requested_bytes: {}
active_leases: {}
roots: {}
{}blockers: {}
",
        report.hard_cap,
        report.soft_watermark,
        report.eviction_target,
        report.buckets.committed,
        report.buckets.staged,
        report.buckets.reserved,
        report.buckets.pending,
        report.admitted,
        report.live_bytes,
        report.reclaimable_bytes,
        report.pinned_bytes,
        report.authorization_revoked_bytes,
        report.deletion_requested_bytes,
        report.active_leases,
        report.roots.len(),
        root_lines(report),
        blocker_list(&report.blockers)
    ) + &session_lines(session)
}

pub fn gc(report: &MaintainReport, dry_run: bool, session: &CacheReport, json: bool) -> String {
    if json {
        return format!(
            "{{\"command\":\"gc\",\"dry_run\":{},\"examined\":{},\"reclaimed_bytes\":{},\"pending_bytes\":{},\"completed_pass\":{},\"deferred\":{},\"blockers\":\"{}\",{}}}",
            bool_json(dry_run),
            report.examined,
            report.reclaimed_bytes,
            report.pending_bytes,
            bool_json(report.completed_pass),
            bool_json(report.deferred),
            blocker_list(&report.blockers),
            session_json(session)
        );
    }
    format!(
        "\
command: gc
dry_run: {}
examined: {}
reclaimed_bytes: {}
pending_bytes: {}
completed_pass: {}
deferred: {}
blockers: {}
{}",
        dry_run,
        report.examined,
        report.reclaimed_bytes,
        report.pending_bytes,
        report.completed_pass,
        report.deferred,
        blocker_list(&report.blockers),
        session_lines(session)
    )
}

pub fn purge(report: &PurgeReport, scope: &str, session: &CacheReport, json: bool) -> String {
    if json {
        return format!(
            "{{\"command\":\"purge\",\"dry_run\":{},\"scope\":\"{}\",\"dropped\":{},\"held_by_lease\":{},{}}}",
            bool_json(report.dry_run),
            escape(scope),
            report.dropped,
            report.held_by_lease,
            session_json(session)
        );
    }
    format!(
        "\
command: purge
dry_run: {}
scope: {scope}
dropped: {}
held_by_lease: {}
{}",
        report.dry_run,
        report.dropped,
        report.held_by_lease,
        session_lines(session)
    )
}

pub fn config_text(policy: &CachePolicy, json: bool) -> String {
    let ttl_days = policy.auto.overlay_ttl_ms / DAY_MS;
    if json {
        return format!(
            "{{\"command\":\"config\",\"auto\":\"{}\",\"max_bytes\":{},\"ttl_days\":{},\"overlay_ttl_ms\":{},\"preview_ttl_ms\":{},\"root_ttl_ms\":{},\"staging_ttl_ms\":{},\"min_interval_ms\":{}}}\n",
            on_off(policy.auto.enabled),
            policy.auto.system_cap_bytes,
            ttl_days,
            policy.auto.overlay_ttl_ms,
            policy.auto.preview_ttl_ms,
            policy.auto.root_ttl_ms,
            policy.auto.staging_ttl_ms,
            policy.auto.min_interval_ms
        );
    }
    format!(
        "\
command: config
auto: {}
max_bytes: {}
ttl_days: {ttl_days}
overlay_ttl_ms: {}
preview_ttl_ms: {}
root_ttl_ms: {}
staging_ttl_ms: {}
min_interval_ms: {}
",
        on_off(policy.auto.enabled),
        policy.auto.system_cap_bytes,
        policy.auto.overlay_ttl_ms,
        policy.auto.preview_ttl_ms,
        policy.auto.root_ttl_ms,
        policy.auto.staging_ttl_ms,
        policy.auto.min_interval_ms
    )
}

pub fn auto_text(report: &AutoReport, json: bool) -> String {
    let last = match report.last_auto_clean_ms {
        Some(stamp) => format!("{stamp} ({})", utc(stamp)),
        None => "none".to_string(),
    };
    if json {
        let last_json = match report.last_auto_clean_ms {
            Some(stamp) => stamp.to_string(),
            None => "null".to_string(),
        };
        return format!(
            "{{\"command\":\"auto\",\"enabled\":{},\"last_auto_clean_ms\":{last_json},\"reclaimed_bytes\":{},\"next_due_ms\":{}}}\n",
            bool_json(report.enabled),
            report.reclaimed_bytes,
            report.next_due_ms
        );
    }
    format!(
        "\
command: auto
enabled: {}
last_auto_clean_ms: {last}
reclaimed_bytes: {}
next_due_ms: {} ({})
",
        report.enabled,
        report.reclaimed_bytes,
        report.next_due_ms,
        utc(report.next_due_ms)
    )
}

/// Epoch milliseconds as an ISO 8601 UTC time to the second, as `2026-11-09T10:04:38Z`.
fn utc(epoch_ms: u64) -> String {
    let seconds = epoch_ms / 1000;
    let (days, of_day) = (seconds / 86_400, seconds % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's days_from_civil inverse), unsigned since stamps are >= 1970.
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}

fn on_off(enabled: bool) -> &'static str {
    if enabled {
        "on"
    } else {
        "off"
    }
}

fn root_lines(report: &Status) -> String {
    let mut out = String::new();
    for root in &report.roots {
        let _ = writeln!(
            out,
            "root {} exclusive={} shared={}",
            root.id.raw(),
            root.exclusive_bytes,
            root.shared_bytes
        );
    }
    out
}

fn session_lines(report: &CacheReport) -> String {
    format!(
        "\
session_committed_bytes: {}
session_reserved_bytes: {}
session_generations: {}
session_pinned_generations: {}
session_reclaimed_bytes: {}
session_blocked_generations: {}
",
        report.committed_bytes,
        report.reserved_bytes,
        report.generations,
        report.pinned_generations,
        report.reclaimed_bytes,
        report.blocked_generations
    )
}

fn session_json(report: &CacheReport) -> String {
    format!(
        "\"session_committed_bytes\":{},\"session_reserved_bytes\":{},\"session_generations\":{},\"session_pinned_generations\":{},\"session_reclaimed_bytes\":{},\"session_blocked_generations\":{}",
        report.committed_bytes,
        report.reserved_bytes,
        report.generations,
        report.pinned_generations,
        report.reclaimed_bytes,
        report.blocked_generations
    )
}

fn blocker_list(blockers: &[Blocker]) -> String {
    if blockers.is_empty() {
        return "none".to_string();
    }
    blockers
        .iter()
        .map(|blocker| match blocker {
            Blocker::Pinned => "pinned",
            Blocker::ActiveLease => "active_lease",
            Blocker::UnavailableGrace => "unavailable_grace",
            Blocker::UnlinkGrace => "unlink_grace",
            Blocker::FailedUnlink => "failed_unlink",
            Blocker::MaintenanceBusy => "maintenance_busy",
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn bool_json(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_adapter_cache::Lifecycle;

    #[test]
    fn status_text_names_the_byte_buckets() {
        let life = Lifecycle::product_default();
        let session = CacheReport::default();
        let text = status(&life.status(), &session, false);
        assert!(text.contains("hard_cap: "));
        assert!(text.contains("live_bytes: "));
        assert!(text.contains("reclaimable_bytes: "));
        assert!(text.contains("pending: "));
        assert!(text.contains("session_reclaimed_bytes: 0"));
    }

    #[test]
    fn utc_formats_epoch_milliseconds() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(utc(1_794_218_678_651), "2026-11-09T10:04:38Z");
        assert_eq!(utc(4_102_444_799_999), "2099-12-31T23:59:59Z");
    }

    #[test]
    fn auto_text_adds_utc_times_and_json_keeps_raw_milliseconds() {
        let report = AutoReport {
            enabled: true,
            last_auto_clean_ms: Some(1_794_218_678_651),
            reclaimed_bytes: 7,
            next_due_ms: 1_794_305_078_651,
        };
        let text = auto_text(&report, false);
        assert!(
            text.contains("last_auto_clean_ms: 1794218678651 (2026-11-09T10:04:38Z)\n"),
            "{text}"
        );
        assert!(
            text.contains("next_due_ms: 1794305078651 (2026-11-10T10:04:38Z)\n"),
            "{text}"
        );
        let json = auto_text(&report, true);
        assert!(
            json.contains("\"last_auto_clean_ms\":1794218678651,"),
            "{json}"
        );
        assert!(json.contains("\"next_due_ms\":1794305078651}"), "{json}");
        assert!(!json.contains('T'), "{json}");
        let never = AutoReport {
            last_auto_clean_ms: None,
            ..report
        };
        assert!(auto_text(&never, false).contains("last_auto_clean_ms: none\n"));
    }
}
