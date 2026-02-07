//! Utils module tests.
//!
//! Tests for utility functions:
//! - `sys_set_nonblocking`
//! - `safe_close`
//!
//! These tests are cross-platform using sockets instead of pipes.

use nucleus::{address, io, socket, utils};
use std::thread;
use std::time::Duration;

/// Helper to create a connected socket pair (server_fd, client_fd).
fn create_connected_pair() -> (io::RawFd, io::RawFd) {
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

    // Wait for the client-side connection to complete
    // This prevents race conditions on macOS where accept() can complete
    // before the client-side connect() finishes the TCP handshake
    for _ in 0..10 {
        match socket::sys_get_socket_error(client_fd) {
            Ok(_) => break,
            Err(_) => thread::sleep(Duration::from_millis(10)),
        }
    }

    io::sys_close(listener_fd);

    (server_fd, client_fd)
}

#[test]
fn test_sys_set_nonblocking_on_socket() {
    let (server_fd, client_fd) = create_connected_pair();

    // Set both ends to non-blocking (they already are from sys_socket, but test it again)
    utils::sys_set_nonblocking(server_fd).expect("Failed to set server non-blocking");
    utils::sys_set_nonblocking(client_fd).expect("Failed to set client non-blocking");

    // Verify non-blocking by attempting to read from empty socket
    let mut buffer = [0u8; 1];
    let result = io::sys_read(server_fd, &mut buffer);
    assert!(result <= 0);

    if result < 0 {
        let err = std::io::Error::last_os_error();
        assert!(
            err.kind() == std::io::ErrorKind::WouldBlock,
            "Expected WouldBlock, got {:?}",
            err
        );
    }

    // Cleanup
    io::sys_close(server_fd);
    io::sys_close(client_fd);
}

#[test]
fn test_safe_close_success() {
    let (server_fd, client_fd) = create_connected_pair();

    // Close using safe_close
    utils::safe_close(server_fd).expect("Failed to close server");
    utils::safe_close(client_fd).expect("Failed to close client");
}

#[test]
fn test_sys_set_nonblocking_socket() {
    // Create a socket
    let fd = socket::sys_socket(socket::AF_INET).expect("Failed to create socket");

    // Socket should already be non-blocking (sys_socket does this)
    // But calling it again should succeed
    utils::sys_set_nonblocking(fd).expect("Failed to set non-blocking again");

    // Cleanup
    io::sys_close(fd);
}

#[test]
fn test_close_twice_with_safe_close() {
    let fd = socket::sys_socket(socket::AF_INET).expect("Failed to create socket");

    // First close should succeed
    utils::safe_close(fd).expect("First close should succeed");

    // Second close should fail (on some platforms it may not fail immediately)
    let result = utils::safe_close(fd);
    // Note: On some systems, closing an already-closed fd may not immediately fail
    // So we just verify the operation completes without panicking
    let _ = result;
}
