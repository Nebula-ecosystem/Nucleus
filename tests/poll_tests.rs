//! Poller module tests.
//!
//! Tests for the kqueue-based poller on macOS:
//! - Poller::new
//! - Poller::waker
//! - Poller::register
//! - Poller::reregister
//! - Poller::deregister
//! - Poller::poll
//! - Waker::wake

use nucleus::{address, io, poll, socket, utils};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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
fn test_poller_register_and_poll_pipe() {
    let mut poller = poll::Poller::new();

    // Create a pipe
    let mut fds = [0i32; 2];
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    // Set non-blocking
    utils::sys_set_nonblocking(read_fd).unwrap();
    utils::sys_set_nonblocking(write_fd).unwrap();

    // Register read_fd for read interest
    let token = 42;
    let interest = poll::Interest::read();
    poller.register(read_fd, token, interest);

    // Write to pipe to make it readable
    let data = b"test";
    let _ = io::sys_write(write_fd, data);

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
    poller.deregister(read_fd);
    io::sys_close(read_fd);
    io::sys_close(write_fd);
}

#[test]
fn test_poller_write_interest() {
    let mut poller = poll::Poller::new();

    // Create a pipe
    let mut fds = [0i32; 2];
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    utils::sys_set_nonblocking(write_fd).unwrap();

    // Register write_fd for write interest
    let token = 100;
    let interest = poll::Interest::write();
    poller.register(write_fd, token, interest);

    // Poll should return writable event (pipe is writable when empty)
    let mut events = Vec::new();
    poller
        .poll(&mut events, Some(Duration::from_millis(100)))
        .expect("Poll failed");

    assert!(!events.is_empty(), "Expected writable event");
    let event = events.iter().find(|e| e.token() == token);
    assert!(event.is_some(), "Expected event for our token");
    assert!(event.unwrap().is_writable());

    // Cleanup
    poller.deregister(write_fd);
    io::sys_close(read_fd);
    io::sys_close(write_fd);
}

#[test]
fn test_poller_timeout() {
    let mut poller = poll::Poller::new();

    // Create a pipe but don't write to it
    let mut fds = [0i32; 2];
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    utils::sys_set_nonblocking(read_fd).unwrap();

    // Register for read (but nothing will be written)
    poller.register(read_fd, 1, poll::Interest::read());

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
    poller.deregister(read_fd);
    io::sys_close(read_fd);
    io::sys_close(write_fd);
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

    // Create a pipe
    let mut fds = [0i32; 2];
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    utils::sys_set_nonblocking(read_fd).unwrap();
    utils::sys_set_nonblocking(write_fd).unwrap();

    // Register for read only with token 1
    poller.register(read_fd, 1, poll::Interest::read());

    // Reregister for write only with token 2 (testing write interest after reregister)
    poller.reregister(write_fd, 2, poll::Interest::write());

    // Poll should return writable event for write_fd with token 2
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
    poller.deregister(read_fd);
    poller.deregister(write_fd);
    io::sys_close(read_fd);
    io::sys_close(write_fd);
}

#[test]
fn test_poller_deregister() {
    let mut poller = poll::Poller::new();

    // Create a pipe
    let mut fds = [0i32; 2];
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    utils::sys_set_nonblocking(read_fd).unwrap();

    // Register
    poller.register(read_fd, 1, poll::Interest::read());

    // Deregister
    poller.deregister(read_fd);

    // Write to pipe
    io::sys_write(write_fd, b"test");

    // Poll should not return event for deregistered fd
    let mut events = Vec::new();
    poller
        .poll(&mut events, Some(Duration::from_millis(50)))
        .expect("Poll failed");

    // No events expected (deregistered)
    assert!(events.is_empty(), "Should have no events after deregister");

    // Cleanup
    io::sys_close(read_fd);
    io::sys_close(write_fd);
}

#[test]
fn test_poller_socket_connect() {
    let mut poller = poll::Poller::new();

    // Create listener
    let listener = socket::sys_socket(libc::AF_INET).unwrap();
    let addr = address::sys_parse_sockaddr("127.0.0.1:0").unwrap();
    socket::sys_bind(listener, &addr.0, addr.1).unwrap();
    socket::sys_listen(listener).unwrap();
    let bound_addr = socket::sys_sockname(listener).unwrap();

    // Create client
    let client = socket::sys_socket(libc::AF_INET).unwrap();

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

    // Create multiple pipes
    let mut pipes = Vec::new();
    for i in 0..5 {
        let mut fds = [0i32; 2];
        unsafe {
            assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
        }
        utils::sys_set_nonblocking(fds[0]).unwrap();
        utils::sys_set_nonblocking(fds[1]).unwrap();
        poller.register(fds[0], i, poll::Interest::read());
        pipes.push((fds[0], fds[1]));
    }

    // Write to some pipes
    io::sys_write(pipes[1].1, b"data");
    io::sys_write(pipes[3].1, b"data");

    // Poll
    let mut events = Vec::new();
    poller
        .poll(&mut events, Some(Duration::from_millis(100)))
        .expect("Poll failed");

    // Should have exactly 2 events
    assert_eq!(events.len(), 2, "Expected 2 events");

    // Check we got the right tokens
    let tokens: Vec<usize> = events.iter().map(|e| e.token()).collect();
    assert!(tokens.contains(&1), "Expected event for pipe 1");
    assert!(tokens.contains(&3), "Expected event for pipe 3");

    // Cleanup
    for (read_fd, write_fd) in pipes {
        poller.deregister(read_fd);
        io::sys_close(read_fd);
        io::sys_close(write_fd);
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
