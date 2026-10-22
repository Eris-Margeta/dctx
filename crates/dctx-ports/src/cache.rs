//! `Cache` port: byte admission before anything is written to managed storage.
//! Implements contracts/API-PORTS-v1.md `Cache.reserve` and CACHE-v1 accounting: every
//! reservation is settled exactly once, by `commit` with the bytes used or by `abort`.
//! Depends on dctx-core `Reservation`. Used by dctx-pack before publishing a snapshot.

use dctx_core::budget::Reservation;
use dctx_core::error::DctxError;

/// Quota keeper for managed storage.
pub trait Cache {
    /// Admits `bytes` or refuses with `CacheLimit`.
    ///
    /// # Errors
    /// Returns `CacheLimit` if the requested bytes exceed the available quota,
    /// or an adapter error if the reservation cannot be recorded.
    fn reserve(&mut self, bytes: u64) -> Result<Reservation, DctxError>;

    /// Settles a reservation with the bytes actually written (at most the reserved bytes).
    ///
    /// # Errors
    /// Returns `InvalidRequest` for an unknown reservation or usage above its
    /// reserved bytes, or an adapter error if settlement fails.
    fn commit(&mut self, reservation: Reservation, used_bytes: u64) -> Result<(), DctxError>;

    /// Releases a reservation whose write did not happen.
    fn abort(&mut self, reservation: Reservation);
}
