// SPDX-License-Identifier: Apache-2.0
//! Bounded streaming fetch with origin, peer and redirect checks.
//! Implements PLUGINS.md §5.3 download policy and I-ORIGIN: check every URL and redirect, bind the check
//! to the connected peer, stop reading past the ceiling, report interruption as a typed failure.
//! Depends on the network port and origin policy; used by install for manifests, signatures and payloads.

use std::io::Read;

use super::Distribution;
use crate::domain::origin::HttpsUrl;
use crate::error::{DistributionError, NetworkError, OriginRefusal};
use crate::ports::Body;

const CHUNK: usize = 64 * 1024;

impl Distribution<'_> {
    /// Streams `url` into `sink`, refusing more than `limit` bytes. Returns the byte count.
    pub(super) fn fetch(
        &self,
        url: &HttpsUrl,
        limit: u64,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), DistributionError>,
    ) -> Result<u64, DistributionError> {
        let policy = &self.policy.origin;
        let mut current = url.clone();
        for _ in 0..=policy.max_redirects {
            policy.check_url(&current)?;
            let response = self.network.get(&current)?;
            policy.check_peer(response.peer)?;
            match response.body {
                Body::Redirect(location) => {
                    let target = HttpsUrl::parse(&location)?;
                    policy.check_redirect(url, &target)?;
                    current = target;
                }
                Body::Content(reader) => return read_bounded(reader, limit, sink),
            }
        }
        Err(OriginRefusal::TooManyRedirects.into())
    }

    /// Fetches a small document completely.
    pub(super) fn fetch_all(&self, url: &HttpsUrl, limit: u64) -> Result<Vec<u8>, DistributionError> {
        let mut out = Vec::new();
        self.fetch(url, limit, &mut |chunk| {
            out.extend_from_slice(chunk);
            Ok(())
        })?;
        Ok(out)
    }
}

fn read_bounded(
    mut reader: Box<dyn Read + Send>,
    limit: u64,
    sink: &mut dyn FnMut(&[u8]) -> Result<(), DistributionError>,
) -> Result<u64, DistributionError> {
    let mut buffer = vec![0u8; CHUNK];
    let mut total: u64 = 0;
    loop {
        let read = reader.read(&mut buffer).map_err(|_| NetworkError::Interrupted)?;
        if read == 0 {
            return Ok(total);
        }
        total = total.saturating_add(read as u64);
        if total > limit {
            return Err(DistributionError::LimitExceeded { limit });
        }
        sink(&buffer[..read])?;
    }
}
