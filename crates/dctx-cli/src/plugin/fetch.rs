// SPDX-License-Identifier: Apache-2.0
//! One bounded HTTPS GET for the discovery index.
//! Implements PLUGINS.md §5.1 search `--refresh`: origin and peer checks, no credential header,
//! no plugin execution. Depends on the distribution network port. Used by search only.

use std::io::Read;

use dctx_plugin_distribution::{DistributionError, HttpsUrl, Network, NetworkError, OriginPolicy, OriginRefusal};

const CHUNK: usize = 64 * 1024;

/// Fetches `url` completely, up to `limit` bytes, following only policy-approved redirects.
pub fn fetch_all(network: &dyn Network, policy: &OriginPolicy, url: &HttpsUrl, limit: u64) -> Result<Vec<u8>, DistributionError> {
    let mut current = url.clone();
    for _ in 0..=policy.max_redirects {
        policy.check_url(&current)?;
        let response = network.get(&current)?;
        policy.check_peer(response.peer)?;
        match response.body {
            dctx_plugin_distribution::Body::Redirect(location) => {
                let target = HttpsUrl::parse(&location)?;
                policy.check_redirect(url, &target)?;
                current = target;
            }
            dctx_plugin_distribution::Body::Content(reader) => return read_bounded(reader, limit),
        }
    }
    Err(OriginRefusal::TooManyRedirects.into())
}

fn read_bounded(mut reader: Box<dyn Read + Send>, limit: u64) -> Result<Vec<u8>, DistributionError> {
    let mut buffer = vec![0u8; CHUNK];
    let mut out = Vec::new();
    loop {
        let read = reader.read(&mut buffer).map_err(|_| NetworkError::Interrupted)?;
        if read == 0 {
            return Ok(out);
        }
        let next = (out.len() as u64).saturating_add(read as u64);
        if next > limit {
            return Err(DistributionError::LimitExceeded { limit });
        }
        out.extend_from_slice(&buffer[..read]);
    }
}
