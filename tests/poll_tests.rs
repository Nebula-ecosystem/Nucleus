//! Poller module tests.
//!
//! Tests for the poller (kqueue on macOS, epoll on Linux, WSAPoll on Windows):
//! - Poller::new
//! - Poller::waker
//! - Poller::register
//! - Poller::reregister
//! - Poller::deregister
//! - Poller::poll
//! - Waker::wake
//!
//! These tests are cross-platform using sockets instead of pipes.

use nucleus::{address, io, poll, socket};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Helper to create a connected socket pair (server_fd, client_fd).
fn create_socket_pair() -> (io::RawFd, io::RawFd) {
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
fn test_poller_new() {
    let poller = poll::Poller::new();
    // Poller should be successfully created
    drop(poller);
}

#[test]
fn test_poller_waker() {
    let poller = poll::Poller::new();
    let waker = poller.waker();

    // Waker should be clonable (Arc)
    let _waker2 = Arc::clone(&waker);

    // Wake should not panic
    waker.wake();
}

#[test]
fn test_poller_register_and_poll_socket() {
    let mut poller = poll::Poller::new();

    // Create a socket pair
    let (server_fd, client_fd) = create_socket_pair();

    // Register server_fd for read interest
    let token = 42;
    let interest = poll::Interest::read();
    poller.register(server_fd, token, interest);

    // Write to client to make server readable
    let data = b"test";
    let _ = io::sys_write(client_fd, data);

    // Small delay for data to arrive
    thread::sleep(Duration::from_millis(10));

    // Poll should return readable event
    let mut events = Vec::new();
    poller
        .poll(&mut events, Some(Duration::from_millis(100)))
        .expect("Poll failed");

    assert!(!events.is_empty(), "Expected at least one event");
    let event = events.iter().find(|e| e.token() == token);
    assert!(event.is_some(), "Expected event for our token");
    assert!(event.unwrap().is_readable());

    // Cleanup
    poller.deregister(server_fd);
    io::sys_close(server_fd);
    io::sys_close(client_fd);
}

#[test]
fn test_poller_write_interest() {
    let mut poller = poll::Poller::new();

    // Create a socket pair
    let (server_fd, client_fd) = create_socket_pair();

    // Register client for write interest
    let token = 100;
    let interest = poll::Interest::write();
    poller.register(client_fd, token, interest);

    // Poll should return writable event (socket is writable when buffer not full)
    let mut events = Vec::new();
    poller
        .poll(&mut events, Some(Duration::from_millis(100)))
        .expect("Poll failed");

    assert!(!events.is_empty(), "Expected writable event");
    let event = events.iter().find(|e| e.token() == token);
    assert!(event.is_some(), "Expected event for our token");
    assert!(event.unwrap().is_writable());

    // Cleanup
    poller.deregister(client_fd);
    io::sys_close(server_fd);
    io::sys_close(client_fd);
}

#[test]
fn test_poller_timeout() {
    let mut poller = poll::Poller::new();

    // Create a socket pair but don't write to it
    let (server_fd, client_fd) = create_socket_pair();

    // Register for read (but nothing will be written)
    poller.register(server_fd, 1, poll::Interest::read());

    // Poll with short timeout - should timeout with no events
    let mut events = Vec::new();
    let start = std::time::Instant::now();
    poller
        .poll(&mut events, Some(Duration::from_millis(50)))
        .expect("Poll failed");
    let elapsed = start.elapsed();

    assert!(events.is_empty(), "Should have no events on timeout");
    assert!(elapsed >= Duration::from_millis(40), "Should have waited");

    // Cleanup
    poller.deregister(server_fd);
    io::sys_close(server_fd);
    io::sys_close(client_fd);
}

#[test]
fn test_poller_wake_from_another_thread() {
    let mut poller = poll::Poller::new();
    let waker = poller.waker();

    // Spawn thread that will wake after a delay
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        waker.wake();
    });

    // Poll with long timeout - should be woken early
    let mut events = Vec::new();
    let start = std::time::Instant::now();
    poller
        .poll(&mut events, Some(Duration::from_secs(5)))
        .expect("Poll failed");
    let elapsed = start.elapsed();

    // Should have returned quickly (much less than 5 seconds)
    assert!(
        elapsed < Duration::from_secs(1),
        "Should have been woken early, but took {:?}",
        elapsed
    );

    handle.join().unwrap();
}

#[test]
fn test_poller_reregister() {
    let mut poller = poll::Poller::new();

    // Create a socket pair
    let (server_fd, client_fd) = create_socket_pair();

    // Register server for read only with token 1
    poller.register(server_fd, 1, poll::Interest::read());

    // Reregister client for write only with token 2
    poller.reregister(client_fd, 2, poll::Interest::write());

    // Poll should return writable event for client_fd with token 2
    let mut events = Vec::new();
    poller
        .poll(&mut events, Some(Duration::from_millis(100)))
        .expect("Poll failed");

    // Should have event with token 2 (write interest)
    assert!(
        !events.is_empty(),
        "Expected writable event after reregister"
    );
    let found = events.iter().any(|e| e.token() == 2 && e.is_writable());
    assert!(
        found,
        "Expected writable event with token 2 after reregister"
    );

    // Cleanup
    poller.deregister(server_fd);
    poller.deregister(client_fd);
    io::sys_close(server_fd);
    io::sys_close(client_fd);
}

#[test]
fn test_poller_deregister() {
    let mut poller = poll::Poller::new();

    // Create a socket pair
    let (server_fd, client_fd) = create_socket_pair();

    // Register
    poller.register(server_fd, 1, poll::Interest::read());

    // Deregister
    poller.deregister(server_fd);

    // Write to client
    io::sys_write(client_fd, b"test");

    // Small delay
    thread::sleep(Duration::from_millis(10));

    // Poll should not return event for deregistered fd
    let mut events = Vec::new();
    poller
        .poll(&mut events, Some(Duration::from_millis(50)))
        .expect("Poll failed");

    // No events expected (deregistered)
    assert!(events.is_empty(), "Should have no events after deregister");

    // Cleanup
    io::sys_close(server_fd);
    io::sys_close(client_fd);
}

#[test]
fn test_poller_socket_connect() {
    let mut poller = poll::Poller::new();

    // Create listener
    let listener = socket::sys_socket(socket::AF_INET).unwrap();
    let addr = address::sys_parse_sockaddr("127.0.0.1:0").unwrap();
    socket::sys_bind(listener, &addr.0, addr.1).unwrap();
    socket::sys_listen(listener).unwrap();
    let bound_addr = socket::sys_sockname(listener).unwrap();

    // Create client
    let client = socket::sys_socket(socket::AF_INET).unwrap();

    // Register client for write (connect completion)
    poller.register(client, 10, poll::Interest::write());

    // Start non-blocking connect
    let _ = socket::sys_connect(client, &bound_addr);

    // Poll for connect completion
    let mut events = Vec::new();
    poller
        .poll(&mut events, Some(Duration::from_millis(500)))
        .expect("Poll failed");

    // Should get writable event when connect completes
    let client_event = events.iter().find(|e| e.token() == 10);
    assert!(client_event.is_some(), "Expected connect completion event");

    // Cleanup
    poller.deregister(client);
    io::sys_close(client);
    io::sys_close(listener);
}

#[test]
fn test_poller_multiple_fds() {
    let mut poller = poll::Poller::new();

    // Create multiple socket pairs
    let mut pairs = Vec::new();
    for i in 0..5 {
        let (server_fd, client_fd) = create_socket_pair();
        poller.register(server_fd, i, poll::Interest::read());
        pairs.push((server_fd, client_fd));
    }

    // Write to some clients
    io::sys_write(pairs[1].1, b"data");
    io::sys_write(pairs[3].1, b"data");

    // Small delay for data to arrive
    thread::sleep(Duration::from_millis(20));

    // Poll
    let mut events = Vec::new();
    poller
        .poll(&mut events, Some(Duration::from_millis(100)))
        .expect("Poll failed");

    // Should have exactly 2 events
    assert_eq!(events.len(), 2, "Expected 2 events");

    // Check we got the right tokens
    let tokens: Vec<usize> = events.iter().map(|e| e.token()).collect();
    assert!(tokens.contains(&1), "Expected event for pair 1");
    assert!(tokens.contains(&3), "Expected event for pair 3");

    // Cleanup
    for (server_fd, client_fd) in pairs {
        poller.deregister(server_fd);
        io::sys_close(server_fd);
        io::sys_close(client_fd);
    }
}

#[test]
fn test_waker_multiple_wakes() {
    let mut poller = poll::Poller::new();
    let waker = poller.waker();

    // Multiple wakes should not cause issues
    for _ in 0..10 {
        waker.wake();
    }

    // Poll should return without blocking
    let mut events = Vec::new();
    let start = std::time::Instant::now();
    poller
        .poll(&mut events, Some(Duration::from_secs(1)))
        .expect("Poll failed");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_millis(100),
        "Should return quickly"
    );
}

#[test]
fn test_poll_no_timeout() {
    let mut poller = poll::Poller::new();
    let waker = poller.waker();

    // Spawn thread to wake immediately
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        waker.wake();
    });

    // Poll with no timeout
    let mut events = Vec::new();
    poller.poll(&mut events, None).expect("Poll failed");

    // Should have returned due to wake
}
