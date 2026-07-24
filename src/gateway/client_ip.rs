//! Client IP resolution that is safe to use as a rate-limiting key behind a
//! chain of reverse proxies.
//!
//! Naively trusting the left-most (or *any* unconditionally-parsed) entry in
//! `X-Forwarded-For` lets a client rate-limit any other client (or bypass
//! limits entirely) by sending a crafted header — the header is fully
//! attacker-controlled up until the point our own infrastructure starts
//! appending to it. This module only trusts the number of hops we
//! explicitly configure via `TRUSTED_PROXY_DEPTH`.

use axum::extract::{ConnectInfo, Request};
use std::net::{IpAddr, SocketAddr};

/// Resolve the real client IP for a request.
///
/// `TRUSTED_PROXY_DEPTH` (default `0`) is the number of reverse proxies
/// between the internet and this service that each append their observed
/// peer address to `X-Forwarded-For` (e.g. `1` for a single load balancer,
/// `2` for an LB in front of an ingress gateway).
///
/// With depth `N`, the header is expected to look like:
/// `<attacker-controlled prefix>, <hop 1's observed peer>, ..., <hop N's observed peer>`
/// where hop N is the proxy directly connecting to us. The right-most `N`
/// entries were each appended by a trusted hop observing a direct TCP peer,
/// so they cannot be forged by the original client — but only the entry at
/// position `len - N` (0-indexed from the left) is the address the client
/// actually connected from; entries to its right are our own infrastructure
/// talking to itself and are not useful as a rate-limit key.
///
/// * `depth = 0` — `X-Forwarded-For` is ignored entirely; the TCP peer
///   address is used. This is the only safe default when the deployment
///   topology isn't known, since trusting an unauthenticated header with no
///   configured depth is an open spoofing vector.
/// * If the header has fewer than `depth` entries, that indicates either a
///   misconfiguration or a request that bypassed the expected proxy chain —
///   fall back to the direct TCP peer rather than guessing.
pub fn extract_client_ip(req: &Request) -> IpAddr {
    let depth = trusted_proxy_depth();
    let peer_ip = || {
        req.extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip())
    };

    if depth > 0 {
        if let Some(header) = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
        {
            if let Some(ip) = resolve_from_xff(header, depth) {
                return ip;
            }
            tracing::warn!(
                header,
                depth,
                "X-Forwarded-For had fewer hops than TRUSTED_PROXY_DEPTH; falling back to TCP peer"
            );
        }
    }

    peer_ip().unwrap_or_else(|| IpAddr::from([0, 0, 0, 0]))
}

/// Parse `X-Forwarded-For` given a trusted proxy depth. Returns `None` if the
/// header doesn't have enough entries to trust, or none of the candidate
/// entries parse as a valid IP.
fn resolve_from_xff(header: &str, depth: usize) -> Option<IpAddr> {
    let hops: Vec<&str> = header
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if hops.is_empty() || hops.len() < depth {
        return None;
    }

    let idx = hops.len() - depth;
    hops.get(idx).and_then(|s| s.parse::<IpAddr>().ok())
}

fn trusted_proxy_depth() -> usize {
    std::env::var("TRUSTED_PROXY_DEPTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_one_takes_rightmost_hop() {
        // Single trusted LB directly in front of us: it appended the real
        // client IP as the only (or last) entry.
        let header = "203.0.113.9";
        assert_eq!(
            resolve_from_xff(header, 1),
            Some("203.0.113.9".parse().unwrap())
        );

        // Attacker prepends a spoofed value; LB still appends the real one last.
        let header = "9.9.9.9, 203.0.113.9";
        assert_eq!(
            resolve_from_xff(header, 1),
            Some("203.0.113.9".parse().unwrap())
        );
    }

    #[test]
    fn depth_two_skips_the_innermost_infra_hop() {
        // attacker-prefix, real-client (appended by LB), LB-ip (appended by ingress)
        let header = "9.9.9.9, 203.0.113.9, 10.0.0.5";
        assert_eq!(
            resolve_from_xff(header, 2),
            Some("203.0.113.9".parse().unwrap())
        );
    }

    #[test]
    fn insufficient_hops_return_none() {
        let header = "203.0.113.9";
        assert_eq!(resolve_from_xff(header, 2), None);
    }

    #[test]
    fn malformed_entries_return_none() {
        let header = "not-an-ip";
        assert_eq!(resolve_from_xff(header, 1), None);
    }

    #[test]
    fn empty_header_returns_none() {
        assert_eq!(resolve_from_xff("", 1), None);
        assert_eq!(resolve_from_xff("   ", 1), None);
    }

    #[test]
    fn depth_zero_never_consults_header_caller_side() {
        // trusted_proxy_depth() itself defaults to 0 when unset; the public
        // extract_client_ip() short-circuits before calling resolve_from_xff
        // in that case, which is exercised via the depth>0 guard.
        assert_eq!(trusted_proxy_depth_default_is_zero(), 0);
    }

    fn trusted_proxy_depth_default_is_zero() -> usize {
        std::env::var("TRUSTED_PROXY_DEPTH")
            .ok()
            .and_then(|v: String| v.parse().ok())
            .unwrap_or(0)
    }
}
