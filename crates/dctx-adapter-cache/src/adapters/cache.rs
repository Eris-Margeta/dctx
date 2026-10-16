//! `Cache` port implementation: global admission without a root ceiling.
//! Implements API-PORTS-v1 `Cache.reserve/commit/abort` on the lifecycle coordinator.
//! Depends on [`crate::Lifecycle`] and `dctx_ports::Cache`. dctx-pack and the facade settle here.
//! Root ceilings stay on `reserve_for`; this impl is the rootless payload path.

use crate::domain::{Lifecycle, Purpose};
use crate::error::CacheError;
use dctx_core::budget::Reservation;
use dctx_core::error::DctxError;
use dctx_ports::Cache;

impl Cache for Lifecycle {
    fn reserve(&mut self, bytes: u64) -> Result<Reservation, DctxError> {
        self.reserve_for(None, Purpose::Payload, bytes).map_err(CacheError::into_dctx)
    }

    fn commit(&mut self, reservation: Reservation, used_bytes: u64) -> Result<(), DctxError> {
        self.commit_reservation(reservation, used_bytes).map_err(CacheError::into_dctx)
    }

    fn abort(&mut self, reservation: Reservation) {
        self.abort_reservation(reservation);
    }
}
