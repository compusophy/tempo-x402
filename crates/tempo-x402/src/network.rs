//! Network validation utilities shared across x402 crates. (updated)
//!
//! Provides IP address classification for SSRF protection in webhooks,
//! proxy validation, and other network-facing components.

use std::net::{Ipv4Addr, Ipv6Addr};

/// Check if an IPv4 address is private, loopback, or otherwise non-routable.
///
/// Covers: loopback (127.0.0.0/8), RFC 1918 private ranges, link-local
/// (169.254.0.0/16), broadcast, unspecified (0.0.0.0), and CGNAT (100.64.0.0/10).
pub fn is_private_ipv4(ip: &Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || (ip.octets()[0] == 100 && (ip.octets()[1] & 0xC0) == 64)
}

/// Check if an IPv6 address is private, loopback, or otherwise non-routable.
///
/// Covers: loopback (::1), unspecified (::), unique local (fc00::/7),
/// link-local (fe80::/10), and IPv4-mapped addresses (delegates to [`is_private_ipv4`]).
pub fn is_private_ipv6(ip: &Ipv6Addr) -> bool {
    ip.is_loopback() || ip.is_unspecified() || {
        let segments = ip.segments();
        (segments[0] & 0xFE00) == 0xFC00
            || (segments[0] & 0xFFC0) == 0xFE80
            || match ip.to_ipv4_mapped() {
                Some(v4) => is_private_ipv4(&v4),
                None => false,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_private_ipv4() {
        assert!(is_private_ipv4(&"127.0.0.1".parse().unwrap()));
        assert!(is_private_ipv4(&"10.0.0.1".parse().unwrap()));
        assert!(is_private_ipv4(&"192.168.1.1".parse().unwrap()));
        assert!(is_private_ipv4(&"172.16.0.1".parse().unwrap()));
        assert!(is_private_ipv4(&"0.0.0.0".parse().unwrap()));
        assert!(is_private_ipv4(&"100.64.0.1".parse().unwrap()));
        assert!(!is_private_ipv4(&"8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn test_private_ipv6() {
        assert!(is_private_ipv6(&"::1".parse().unwrap()));
        assert!(is_private_ipv6(&"::".parse().unwrap()));
        assert!(is_private_ipv6(&"fc00::1".parse().unwrap()));
        assert!(is_private_ipv6(&"fe80::1".parse().unwrap()));
        assert!(!is_private_ipv6(&"2001:db8::1".parse().unwrap()));
    }
}
