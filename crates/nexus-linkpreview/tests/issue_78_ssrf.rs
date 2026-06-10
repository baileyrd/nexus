//! Regression tests for issue #78 — SSRF guard in `nexus-linkpreview`.
//!
//! The fetcher had no host filter; `http://169.254.169.254/...`
//! (AWS EC2 metadata IP), `127.0.0.1`, RFC1918 / link-local /
//! loopback / IPv6 ULA — all reachable. Reqwest's default 10-redirect
//! follow also meant a public initial URL could redirect to one of
//! these. Plus `resp.text()` decoded the entire body before the
//! 512 KiB substring cap, so a server streaming a multi-gigabyte
//! body could OOM the host.
//!
//! These tests cover the pure SSRF guard helper exhaustively. The
//! redirect path and the streaming-cap fix are exercised by the
//! wider integration smoke (production fetch); we don't stand up
//! an HTTP server here because the audit's primary concern is the
//! address-class denylist, and that's a pure function.
//!
//! Redirects are now followed in-crate with per-hop validation, and
//! each hop's connection is DNS-pinned to the IP that passed this
//! guard (`ClientBuilder::resolve`), closing the rebinding TOCTOU
//! flagged as review item V13. The pinning plumbing is unit-tested
//! in `src/lib.rs` (`dns_pin_*` tests) without real DNS.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use nexus_linkpreview::is_blocked_address;

#[test]
fn ipv4_loopback_is_blocked() {
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(127, 1, 2, 3))));
}

#[test]
fn ipv4_aws_metadata_is_blocked() {
    // AWS EC2 instance metadata service.
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
        169, 254, 169, 254
    ))));
}

#[test]
fn ipv4_link_local_is_blocked() {
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
        169, 254, 0, 1
    ))));
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
        169, 254, 255, 254
    ))));
}

#[test]
fn ipv4_rfc1918_is_blocked() {
    // 10.0.0.0/8
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
        10, 255, 255, 254
    ))));
    // 172.16.0.0/12
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
        172, 31, 255, 254
    ))));
    // 192.168.0.0/16
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
        192, 168, 0, 1
    ))));
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
        192, 168, 1, 1
    ))));
}

#[test]
fn ipv4_cgnat_is_blocked() {
    // RFC6598 carrier-grade NAT (100.64.0.0/10).
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(
        100, 127, 255, 254
    ))));
}

#[test]
fn ipv4_unspecified_and_broadcast_are_blocked() {
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::BROADCAST)));
    // 0.0.0.0/8 — "this network" reserved.
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 1))));
}

#[test]
fn ipv4_multicast_is_blocked() {
    assert!(is_blocked_address(IpAddr::V4(Ipv4Addr::new(224, 0, 0, 1))));
}

#[test]
fn ipv6_loopback_unspecified_multicast_blocked() {
    assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::LOCALHOST)));
    assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
    assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::new(
        0xff02, 0, 0, 0, 0, 0, 0, 1
    ))));
}

#[test]
fn ipv6_unique_local_is_blocked() {
    // fc00::/7 — Unique Local Addresses (RFC4193).
    assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::new(
        0xfc00, 0, 0, 0, 0, 0, 0, 1
    ))));
    assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::new(
        0xfd12, 0, 0, 0, 0, 0, 0, 1
    ))));
}

#[test]
fn ipv6_link_local_is_blocked() {
    // fe80::/10 — link-local.
    assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::new(
        0xfe80, 0, 0, 0, 0, 0, 0, 1
    ))));
    assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::new(
        0xfebf, 0xffff, 0xffff, 0xffff, 0, 0, 0, 1
    ))));
}

#[test]
fn ipv6_mapped_smuggling_is_blocked() {
    // ::ffff:127.0.0.1 — IPv4-mapped IPv6 of loopback. An attacker
    // can't bypass the v4 deny list by smuggling the address in v6
    // form; the helper recurses into the v4 check.
    assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::new(
        0, 0, 0, 0, 0, 0xffff, 0x7f00, 0x0001
    ))));
    // ::ffff:169.254.169.254 — AWS metadata via mapped form.
    assert!(is_blocked_address(IpAddr::V6(Ipv6Addr::new(
        0, 0, 0, 0, 0, 0xffff, 0xa9fe, 0xa9fe
    ))));
}

#[test]
fn public_ipv4_is_allowed() {
    // Public DNS (Google + Cloudflare).
    assert!(!is_blocked_address(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
    assert!(!is_blocked_address(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
    // 172.32.x.x — outside the 172.16/12 RFC1918 block.
    assert!(!is_blocked_address(IpAddr::V4(Ipv4Addr::new(
        172, 32, 0, 1
    ))));
}

#[test]
fn public_ipv6_is_allowed() {
    // Google IPv6 DNS.
    assert!(!is_blocked_address(IpAddr::V6(Ipv6Addr::new(
        0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888
    ))));
}
