//! Socket module tests.
//!
//! Tests for TCP socket lifecycle and options:
//! - `sys_socket`
//! - `sys_bind`
//! - `sys_listen`
//! - `sys_accept`
//! - `sys_connect`
//! - `sys_shutdown`
//! - Socket options

use nucleus::{address, io, socket};
use std::thread;
use std::time::Duration;

#[test]
fn test_sys_socket_creates_valid_fd() {
    let fd = socket::sys_socket(socket::AF_INET).expect("Failed to create socket");
    #[cfg(unix)]
    assert!(fd >= 0, "Socket fd should be non-negative");
    #[cfg(windows)]
    assert!(fd != 0, "Socket fd should be valid");

    // Cleanup
    io::sys_close(fd);
}

#[test]
fn test_sys_socket_ipv6() {
    let fd = socket::sys_socket(socket::AF_INET6).expect("Failed to create IPv6 socket");
    #[cfg(unix)]
    assert!(fd >= 0, "Socket fd should be non-negative");
    #[cfg(windows)]
    assert!(fd != 0, "Socket fd should be valid");

    // Cleanup
    io::sys_close(fd);
}

#[test]
fn test_sys_bind_to_ephemeral_port() {
    let fd = socket::sys_socket(socket::AF_INET).expect("Failed to create socket");

    // Bind to 127.0.0.1:0 (ephemeral port)
    let (storage, len) = address::sys_parse_sockaddr("127.0.0.1:0").expect("Failed to parse addr");
    socket::sys_bind(fd, &storage, len).expect("Failed to bind socket");

    // Get the actual bound address
    let bound_addr = socket::sys_sockname(fd).expect("Failed to get sockname");
    assert!(bound_addr.port() > 0, "Should have assigned a port");

    // Cleanup
    io::sys_close(fd);
}

#[test]
fn test_sys_listen() {
    let fd = socket::sys_socket(socket::AF_INET).expect("Failed to create socket");

    let (storage, len) = address::sys_parse_sockaddr("127.0.0.1:0").expect("Failed to parse addr");
    socket::sys_bind(fd, &storage, len).expect("Failed to bind socket");

    socket::sys_listen(fd).expect("Failed to listen");

    // Cleanup
    io::sys_close(fd);
}

#[test]
fn test_sys_connect_and_accept() {
    // Create listener
    let listener_fd =
        socket::sys_socket(socket::AF_INET).expect("Failed to create listener socket");
    let (storage, len) = address::sys_parse_sockaddr("127.0.0.1:0").expect("Failed to parse addr");
    socket::sys_bind(listener_fd, &storage, len).expect("Failed to bind listener");
    socket::sys_listen(listener_fd).expect("Failed to listen");

    let listener_addr = socket::sys_sockname(listener_fd).expect("Failed to get sockname");

    // Create client in a thread
    let connect_addr = listener_addr;
    let client_thread = thread::spawn(move || {
        let client_fd =
            socket::sys_socket(socket::AF_INET).expect("Failed to create client socket");

        // Connect (may return EINPROGRESS/WSAEWOULDBLOCK for non-blocking)
        let result = socket::sys_connect(client_fd, &connect_addr);

        // For non-blocking, EINPROGRESS/WouldBlock is expected
        if let Err(ref e) = result {
            let is_in_progress = e.kind() == std::io::ErrorKind::WouldBlock;
            #[cfg(unix)]
            let is_in_progress = is_in_progress || e.raw_os_error() == Some(libc::EINPROGRESS);
            if !is_in_progress {
                result.expect("Connect failed unexpectedly");
            }
        }

        // Wait a bit for connection to complete
        thread::sleep(Duration::from_millis(50));

        // Verify connection succeeded
        socket::sys_get_socket_error(client_fd).expect("Socket should have no error after connect");

        client_fd
    });

    // Accept connection on listener
    thread::sleep(Duration::from_millis(100)); // Give time for connect

    let (accepted_fd, peer_addr) = socket::sys_accept(listener_fd).expect("Failed to accept");
    #[cfg(unix)]
    assert!(accepted_fd >= 0);
    #[cfg(windows)]
    assert!(accepted_fd != 0);
    assert_eq!(
        peer_addr.ip(),
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
    );

    let client_fd = client_thread.join().expect("Client thread panicked");

    // Cleanup
    io::sys_close(accepted_fd);
    io::sys_close(client_fd);
    io::sys_close(listener_fd);
}

#[test]
fn test_sys_set_reuseaddr() {
    let fd = socket::sys_socket(socket::AF_INET).expect("Failed to create socket");

    // Set SO_REUSEADDR
    socket::sys_set_reuseaddr(fd).expect("Failed to set reuseaddr");

    // Cleanup
    io::sys_close(fd);
}

#[test]
fn test_sys_set_v6only() {
    let fd = socket::sys_socket(socket::AF_INET6).expect("Failed to create IPv6 socket");

    // Set IPV6_V6ONLY
    socket::sys_set_v6only(fd, true).expect("Failed to set v6only");

    // Cleanup
    io::sys_close(fd);
}

#[test]
fn test_sys_get_socket_error() {
    let fd = socket::sys_socket(socket::AF_INET).expect("Failed to create socket");

    // Fresh socket should have no error
    socket::sys_get_socket_error(fd).expect("Fresh socket should have no error");

    // Cleanup
    io::sys_close(fd);
}

#[test]
fn test_sys_shutdown() {
    // Create a connected pair
    let listener_fd = socket::sys_socket(socket::AF_INET).expect("Failed to create listener");
    let (storage, len) = address::sys_parse_sockaddr("127.0.0.1:0").expect("Failed to parse addr");
    socket::sys_bind(listener_fd, &storage, len).expect("Failed to bind");
    socket::sys_listen(listener_fd).expect("Failed to listen");

    let addr = socket::sys_sockname(listener_fd).expect("Failed to get sockname");

    let client_fd = socket::sys_socket(socket::AF_INET).expect("Failed to create client");
    let _ = socket::sys_connect(client_fd, &addr); // May return EINPROGRESS/WouldBlock

    // Accept (retry loop for non-blocking accept)
    let server_fd = loop {
        match socket::sys_accept(listener_fd) {
            Ok((fd, _)) => break fd,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("Accept failed: {:?}", e),
        }
    };

    // Wait for the client-side connection to complete by checking socket error
    // This is necessary on macOS where the server accept() can complete before
    // the client-side connect() finishes the TCP handshake
    for _ in 0..10 {
        match socket::sys_get_socket_error(client_fd) {
            Ok(_) => break, // Connection established
            Err(_) => thread::sleep(Duration::from_millis(10)),
        }
    }

    // Verify connection is established
    socket::sys_get_socket_error(client_fd).expect("Client connection should be established");

    // Shutdown the server's write side
    socket::sys_shutdown(server_fd, std::net::Shutdown::Write).expect("Failed to shutdown write");

    // Shutdown the client's read side
    socket::sys_shutdown(client_fd, std::net::Shutdown::Read).expect("Failed to shutdown read");

    // Cleanup
    io::sys_close(server_fd);
    io::sys_close(client_fd);
    io::sys_close(listener_fd);
}

#[test]
fn test_sys_ipv6_is_necessary() {
    let fd = socket::sys_socket(socket::AF_INET6).expect("Failed to create IPv6 socket");

    // IPv6 addresses need IPv6-specific handling
    let _result = socket::sys_ipv6_is_necessary(fd, socket::AF_INET6);

    // Just verify it doesn't crash
    io::sys_close(fd);
}

#[test]
fn test_bind_already_in_use() {
    // Bind to a port
    let fd1 = socket::sys_socket(socket::AF_INET).expect("Failed to create socket 1");
    let (storage, len) = address::sys_parse_sockaddr("127.0.0.1:0").expect("Failed to parse addr");
    socket::sys_bind(fd1, &storage, len).expect("Failed to bind first socket");

    let bound_addr = socket::sys_sockname(fd1).expect("Failed to get sockname");

    // Try to bind another socket to the same address without SO_REUSEADDR
    let fd2 = socket::sys_socket(socket::AF_INET).expect("Failed to create socket 2");
    let (storage2, len2) = address::socketaddr_to_storage(&bound_addr);

    let result = socket::sys_bind(fd2, &storage2, len2);
    assert!(result.is_err(), "Binding to same address should fail");

    // Cleanup
    io::sys_close(fd1);
    io::sys_close(fd2);
}
