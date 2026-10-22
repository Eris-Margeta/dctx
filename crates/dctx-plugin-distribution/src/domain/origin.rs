//! HTTPS locators and the installer's download-origin policy.
//! Implements PLUGINS.md §5.3 and distribution.md I-ORIGIN: HTTPS only, no userinfo or fragment,
//! no loopback/private/link-local destination unless policy permits, redirects stay on the selected
//! origin or an approved host, checks bind to the connected peer address, artifacts resolve beside the manifest.
//! Pure; depends on names. Used by the download service and the discovery index.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::names::SafeFileName;
use crate::error::OriginRefusal;

const MAX_URL: usize = 2048;

/// A parsed `https://host[:port]/path[?query]` URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpsUrl {
    host: String,
    port: Option<u16>,
    path: String,
}

impl HttpsUrl {
    /// Parses and checks the URL grammar the installer accepts.
    /// # Errors
    /// Refuses malformed URLs, non-HTTPS schemes, fragments, userinfo, or invalid host/port syntax.
    pub fn parse(text: &str) -> Result<Self, OriginRefusal> {
        if text.len() > MAX_URL || text.bytes().any(|b| b <= b' ' || b >= 0x7f || b == b'\\') {
            return Err(OriginRefusal::Syntax);
        }
        let rest = text.strip_prefix("https://").ok_or(OriginRefusal::Scheme)?;
        if text.contains('#') {
            return Err(OriginRefusal::Fragment);
        }
        let split = rest.find(['/', '?']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(split);
        if authority.contains('@') {
            return Err(OriginRefusal::Userinfo);
        }
        let (host, port) = split_port(authority)?;
        let host = host.to_ascii_lowercase();
        let valid_host = !host.is_empty()
            && match host.strip_prefix('[') {
                Some(literal) => literal.strip_suffix(']').is_some_and(|inner| inner.parse::<Ipv6Addr>().is_ok()),
                None => host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.'),
            };
        if !valid_host {
            return Err(OriginRefusal::Syntax);
        }
        let path = if tail.starts_with('/') { tail.to_owned() } else { format!("/{tail}") };
        Ok(Self { host, port, path })
    }

    /// Lowercase host, IPv6 literals keep their brackets.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// `host[:port]`, the unit of origin comparison.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn origin(&self) -> String {
        match self.port {
            Some(port) => format!("{}:{port}", self.host),
            None => self.host.clone(),
        }
    }

    /// The URL of `file` in the same directory as this URL.
    #[allow(clippy::must_use_candidate, clippy::return_self_not_must_use, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn sibling(&self, file: &SafeFileName) -> Self {
        let path = self.path.split('?').next().unwrap_or("/");
        let directory = &path[..path.rfind('/').map_or(0, |index| index + 1)];
        Self { host: self.host.clone(), port: self.port, path: format!("{directory}{}", file.as_str()) }
    }
}

impl std::fmt::Display for HttpsUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "https://{}{}", self.origin(), self.path)
    }
}

fn split_port(authority: &str) -> Result<(&str, Option<u16>), OriginRefusal> {
    let colon = if authority.starts_with('[') {
        authority.find(']').map(|end| end + 1).filter(|&end| end < authority.len())
    } else {
        authority.rfind(':')
    };
    match colon {
        Some(index) if authority.as_bytes()[index] == b':' => {
            let port = authority[index + 1..].parse::<u16>().map_err(|_| OriginRefusal::Syntax)?;
            Ok((&authority[..index], Some(port)))
        }
        Some(_) => Err(OriginRefusal::Syntax),
        None => Ok((authority, None)),
    }
}

/// True for loopback, private, link-local, CGNAT, unspecified, multicast and unique-local addresses.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn is_private_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => private_v4(v4),
        IpAddr::V6(v6) => private_v6(v6),
    }
}

fn private_v4(v4: Ipv4Addr) -> bool {
    let [a, b, ..] = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || v4.is_multicast()
        || a == 0
        || (a == 100 && (64..128).contains(&b))
}

fn private_v6(v6: Ipv6Addr) -> bool {
    if let Some(mapped) = v6.to_ipv4_mapped() {
        return private_v4(mapped);
    }
    let first = v6.segments()[0];
    v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
}

/// Installer network policy; it is not a plugin runtime grant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginPolicy {
    /// Explicit installation-policy exception for private destinations.
    pub allow_private: bool,
    /// Origins (`host[:port]`) a redirect may additionally reach.
    pub approved_redirects: Vec<String>,
    /// Redirect hops followed per request.
    pub max_redirects: u8,
}

impl Default for OriginPolicy {
    fn default() -> Self {
        Self { allow_private: false, approved_redirects: Vec::new(), max_redirects: 5 }
    }
}

impl OriginPolicy {
    /// Checks a URL's host before connecting.
    /// # Errors
    /// Refuses private destination hosts when private destinations are not allowed.
    pub fn check_url(&self, url: &HttpsUrl) -> Result<(), OriginRefusal> {
        if self.allow_private {
            return Ok(());
        }
        let host = url.host();
        let literal = host.trim_start_matches('[').trim_end_matches(']');
        let private = host == "localhost"
            || host.ends_with(".localhost")
            || literal.parse::<IpAddr>().is_ok_and(is_private_address);
        if private { Err(OriginRefusal::PrivateDestination) } else { Ok(()) }
    }

    /// Checks the address the connection actually reached (binds DNS to the connection).
    /// # Errors
    /// Refuses private peer addresses when private destinations are not allowed.
    pub fn check_peer(&self, peer: IpAddr) -> Result<(), OriginRefusal> {
        if !self.allow_private && is_private_address(peer) {
            return Err(OriginRefusal::PrivateDestination);
        }
        Ok(())
    }

    /// Checks a redirect from the selected origin `selected` to `target`.
    /// # Errors
    /// Refuses a disallowed destination or an origin outside the selected/approved redirect origins.
    pub fn check_redirect(&self, selected: &HttpsUrl, target: &HttpsUrl) -> Result<(), OriginRefusal> {
        self.check_url(target)?;
        let origin = target.origin();
        if origin == selected.origin() || self.approved_redirects.contains(&origin) {
            Ok(())
        } else {
            Err(OriginRefusal::NotApproved)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_https_and_resolves_siblings() {
        let url = HttpsUrl::parse("https://Example.org/rel/1.0.0/plugin.json").unwrap();
        assert_eq!(url.origin(), "example.org");
        let file = SafeFileName::parse("abc.bin").unwrap();
        assert_eq!(url.sibling(&file).to_string(), "https://example.org/rel/1.0.0/abc.bin");
        let port = HttpsUrl::parse("https://example.org:8443?x=1").unwrap();
        assert_eq!(port.origin(), "example.org:8443");
        assert_eq!(port.sibling(&file).to_string(), "https://example.org:8443/abc.bin");
        assert_eq!(HttpsUrl::parse("https://[2001:db8::1]:443/a").unwrap().host(), "[2001:db8::1]");
    }

    #[test]
    fn refuses_scheme_userinfo_fragment_and_syntax() {
        assert_eq!(HttpsUrl::parse("http://example.org/p"), Err(OriginRefusal::Scheme));
        assert_eq!(HttpsUrl::parse("file:///etc/passwd"), Err(OriginRefusal::Scheme));
        assert_eq!(HttpsUrl::parse("https://user:pw@example.org/p"), Err(OriginRefusal::Userinfo));
        assert_eq!(HttpsUrl::parse("https://example.org/p#x"), Err(OriginRefusal::Fragment));
        for bad in ["https://", "https://exa mple.org/", "https://example.org:99999/", "https://a_b.org/", "https://x\\y/"] {
            assert_eq!(HttpsUrl::parse(bad), Err(OriginRefusal::Syntax), "{bad}");
        }
    }

    #[test]
    fn private_destinations_need_explicit_policy() {
        let policy = OriginPolicy::default();
        for host in ["localhost", "a.localhost", "127.0.0.1", "10.1.2.3", "169.254.169.254", "192.168.1.1", "100.64.0.1", "[::1]", "[fe80::1]", "[fd00::1]", "[::ffff:127.0.0.1]", "0.0.0.0"] {
            let url = HttpsUrl::parse(&format!("https://{host}/p")).unwrap();
            assert_eq!(policy.check_url(&url), Err(OriginRefusal::PrivateDestination), "{host}");
        }
        let public = HttpsUrl::parse("https://203.0.113.9/p").unwrap();
        assert!(policy.check_url(&public).is_ok());
        assert!(policy.check_peer("10.0.0.1".parse().unwrap()).is_err());
        let open = OriginPolicy { allow_private: true, ..OriginPolicy::default() };
        assert!(open.check_peer("10.0.0.1".parse().unwrap()).is_ok());
    }

    #[test]
    fn redirects_stay_on_origin_or_approved_hosts() {
        let selected = HttpsUrl::parse("https://releases.example.org/a/plugin.json").unwrap();
        let same = HttpsUrl::parse("https://releases.example.org/b").unwrap();
        let cdn = HttpsUrl::parse("https://cdn.example.net/b").unwrap();
        let policy = OriginPolicy::default();
        assert!(policy.check_redirect(&selected, &same).is_ok());
        assert_eq!(policy.check_redirect(&selected, &cdn), Err(OriginRefusal::NotApproved));
        let approved = OriginPolicy { approved_redirects: vec!["cdn.example.net".into()], ..OriginPolicy::default() };
        assert!(approved.check_redirect(&selected, &cdn).is_ok());
    }
}
