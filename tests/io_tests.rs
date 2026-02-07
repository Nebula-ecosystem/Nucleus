//! I/O module tests.
//!
//! Tests for low-level file descriptor I/O operations:
//! - `sys_read`
//! - `sys_write`
//! - `sys_close`
//!
//! These tests use TCP sockets for cross-platform compatibility.

use nucleus::{address, io, socket};
use std::thread;
use std::time::Duration;

/// Helper to create a connected socket pair (server_fd, client_fd).
fn create_connected_pair() -> (io::RawFd, io::RawFd) {
    // Create listener
    let listener_fd = socket::sys_socket(socket::AF_INET).expect("Failed to create listener");
    let (storage, len) = address::sys_parse_sockaddr("127.0.0.1:0").expect("Failed to parse addr");
    socket::sys_bind(listener_fd, &storage, len).expect("Failed to bind");
    socket::sys_listen(listener_fd).expect("Failed to listen");

    let addr = socket::sys_sockname(listener_fd).expect("Failed to get sockname");

    // Create client and connect
    let client_fd = socket::sys_socket(socket::AF_INET).expect("Failed to create client");
    let connect_result = socket::sys_connect(client_fd, &addr);

    // For non-blocking sockets, EINPROGRESS/WSAEWOULDBLOCK is expected
    if let Err(ref e) = connect_result {
        let is_in_progress = e.kind() == std::io::ErrorKind::WouldBlock;
        #[cfg(unix)]
        let is_in_progress = is_in_progress || e.raw_os_error() == Some(libc::EINPROGRESS);
        if !is_in_progress {
            connect_result.expect("Connect failed unexpectedly");
        }
    }

    // Wait for connection
    thread::sleep(Duration::from_millis(50));

    // Accept
    let (server_fd, _) = socket::sys_accept(listener_fd).expect("Failed to accept");

    // Close listener
    io::sys_close(listener_fd);

    (server_fd, client_fd)
}

#[test]
fn test_sys_read_from_socket() {
    let (server_fd, client_fd) = create_connected_pair();

    // Write data from client
    let data = b"Hello, Nucleus!";
    let written = io::sys_write(client_fd, data);
    assert!(written > 0, "Write should succeed");

    // Wait for data to arrive
    thread::sleep(Duration::from_millis(50));

    // Read from server
    let mut buffer = [0u8; 64];
    let bytes_read = io::sys_read(server_fd, &mut buffer);
    assert!(bytes_read > 0, "Read should succeed");
    assert_eq!(&buffer[..bytes_read as usize], data);

    // Cleanup
    io::sys_close(server_fd);
    io::sys_close(client_fd);
}

#[test]
fn test_sys_write_to_socket() {
    let (server_fd, client_fd) = create_connected_pair();

    // Write data from server
    let data = b"Test data for sys_write";
    let bytes_written = io::sys_write(server_fd, data);
    assert_eq!(bytes_written as usize, data.len());

    // Wait for data to arrive
    thread::sleep(Duration::from_millis(50));

    // Read from client
    let mut buffer = [0u8; 64];
    let bytes_read = io::sys_read(client_fd, &mut buffer);
    assert!(bytes_read > 0, "Read should succeed");
    assert_eq!(&buffer[..bytes_read as usize], data);

    // Cleanup
    io::sys_close(server_fd);
    io::sys_close(client_fd);
}

#[test]
fn test_sys_read_empty_socket_returns_wouldblock() {
    let (server_fd, client_fd) = create_connected_pair();

    // Try to read from socket with no data - should return -1 with EAGAIN/WouldBlock
    let mut buffer = [0u8; 64];
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
fn test_sys_close_socket() {
    let (server_fd, client_fd) = create_connected_pair();

    // Close both ends
    io::sys_close(server_fd);
    io::sys_close(client_fd);

    // Verify they're closed by trying to read (should fail)
    let mut buffer = [0u8; 1];
    let result = io::sys_read(server_fd, &mut buffer);
    assert!(result <= 0);
}

#[test]
fn test_sys_read_write_large_data() {
    let (server_fd, client_fd) = create_connected_pair();

    // Write data
    let data: Vec<u8> = (0u8..255).cycle().take(4096).collect();
    let mut total_written = 0;

    // Write as much as possible
    while total_written < data.len() {
        let written = io::sys_write(client_fd, &data[total_written..]);
        if written > 0 {
            total_written += written as usize;
        } else {
            break;
        }
    }

    // Wait for data to arrive
    thread::sleep(Duration::from_millis(100));

    // Read it all back
    let mut buffer = vec![0u8; total_written];
    let mut total_read = 0;

    while total_read < total_written {
        let read = io::sys_read(server_fd, &mut buffer[total_read..]);
        if read > 0 {
            total_read += read as usize;
        } else if read == 0 {
            break;
        } else {
            // WouldBlock, wait a bit
            thread::sleep(Duration::from_millis(10));
        }
    }

    assert_eq!(total_read, total_written);
    assert_eq!(&buffer[..total_read], &data[..total_written]);

    // Cleanup
    io::sys_close(server_fd);
    io::sys_close(client_fd);
}
