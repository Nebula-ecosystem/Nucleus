//! Address conversion tests.
//!
//! Tests for socket address conversions:
//! - `sys_parse_sockaddr`
//! - `sockaddr_storage_to_socketaddr`
//! - `socketaddr_to_storage`

use nucleus::address;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

#[test]
fn test_sys_parse_sockaddr_ipv4() {
    let (storage, len) =
        address::sys_parse_sockaddr("127.0.0.1:8080").expect("Failed to parse IPv4 addr");

    assert!(len > 0);

    // Convert back and verify
    let addr = address::sockaddr_storage_to_socketaddr(&storage).expect("Failed to convert back");
    assert_eq!(addr, SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080));
}

#[test]
fn test_sys_parse_sockaddr_ipv6() {
    let (storage, len) =
        address::sys_parse_sockaddr("[::1]:443").expect("Failed to parse IPv6 addr");

    assert!(len > 0);

    // Convert back and verify
    let addr = address::sockaddr_storage_to_socketaddr(&storage).expect("Failed to convert back");
    assert_eq!(addr.ip(), IpAddr::V6(Ipv6Addr::LOCALHOST));
    assert_eq!(addr.port(), 443);
}

#[test]
fn test_sys_parse_sockaddr_invalid() {
    // Invalid address should fail
    let result = address::sys_parse_sockaddr("not-an-address");
    assert!(result.is_err());

    let result = address::sys_parse_sockaddr("");
    assert!(result.is_err());

    let result = address::sys_parse_sockaddr("127.0.0.1"); // Missing port
    assert!(result.is_err());
}

#[test]
fn test_socketaddr_to_storage_ipv4() {
    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 3000));

    let (storage, len) = address::socketaddr_to_storage(&addr);
    assert!(len > 0);

    // Convert back
    let converted =
        address::sockaddr_storage_to_socketaddr(&storage).expect("Failed to convert back");
    assert_eq!(converted, addr);
}

#[test]
fn test_socketaddr_to_storage_ipv6() {
    let addr = SocketAddr::V6(SocketAddrV6::new(
        Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1),
        9000,
        0,
        0,
    ));

    let (storage, len) = address::socketaddr_to_storage(&addr);
    assert!(len > 0);

    // Convert back
    let converted =
        address::sockaddr_storage_to_socketaddr(&storage).expect("Failed to convert back");
    assert_eq!(converted.ip(), addr.ip());
    assert_eq!(converted.port(), addr.port());
}

#[test]
fn test_socketaddr_roundtrip_all_zeros() {
    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0));

    let (storage, _len) = address::socketaddr_to_storage(&addr);
    let converted =
        address::sockaddr_storage_to_socketaddr(&storage).expect("Failed to convert back");

    assert_eq!(converted, addr);
}

#[test]
fn test_socketaddr_roundtrip_broadcast() {
    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, 65535));

    let (storage, _len) = address::socketaddr_to_storage(&addr);
    let converted =
        address::sockaddr_storage_to_socketaddr(&storage).expect("Failed to convert back");

    assert_eq!(converted, addr);
}

#[test]
fn test_socketaddr_roundtrip_ipv6_full() {
    let addr = SocketAddr::V6(SocketAddrV6::new(
        Ipv6Addr::new(0x2001, 0x0db8, 0x85a3, 0, 0, 0x8a2e, 0x0370, 0x7334),
        8080,
        0,
        0,
    ));

    let (storage, _len) = address::socketaddr_to_storage(&addr);
    let converted =
        address::sockaddr_storage_to_socketaddr(&storage).expect("Failed to convert back");

    assert_eq!(converted.ip(), addr.ip());
    assert_eq!(converted.port(), addr.port());
}

#[test]
fn test_parse_addresses_various_ports() {
    // Test various port numbers
    for port in [0u16, 1, 80, 443, 8080, 49152, 65535] {
        let addr_str = format!("127.0.0.1:{}", port);
        let (storage, _len) = address::sys_parse_sockaddr(&addr_str).expect("Failed to parse addr");
        let converted =
            address::sockaddr_storage_to_socketaddr(&storage).expect("Failed to convert");
        assert_eq!(converted.port(), port);
    }
}

#[test]
fn test_parse_addresses_various_ipv4() {
    let test_ips = [
        "0.0.0.0",
        "127.0.0.1",
        "192.168.0.1",
        "10.0.0.1",
        "255.255.255.255",
    ];

    for ip in test_ips {
        let addr_str = format!("{}:8080", ip);
        let (storage, _len) = address::sys_parse_sockaddr(&addr_str).expect("Failed to parse addr");
        let converted =
            address::sockaddr_storage_to_socketaddr(&storage).expect("Failed to convert");

        let expected_ip: Ipv4Addr = ip.parse().unwrap();
        assert_eq!(converted.ip(), IpAddr::V4(expected_ip));
    }
}

#[test]
fn test_parse_addresses_various_ipv6() {
    let test_addrs = [
        "[::]:80",
        "[::1]:443",
        "[fe80::1]:8080",
        "[::ffff:127.0.0.1]:9000",
    ];

    for addr_str in test_addrs {
        let result = address::sys_parse_sockaddr(addr_str);
        assert!(result.is_ok(), "Failed to parse {}", addr_str);
    }
}
