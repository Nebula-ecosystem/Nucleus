//! Utils module tests.
//!
//! Tests for utility functions:
//! - `sys_set_nonblocking`
//! - `safe_close`

use nucleus::{io, utils};

#[test]
fn test_sys_set_nonblocking_on_pipe() {
    // Create a pipe
    let mut fds = [0i32; 2];
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    // Set both ends to non-blocking
    utils::sys_set_nonblocking(read_fd).expect("Failed to set read end non-blocking");
    utils::sys_set_nonblocking(write_fd).expect("Failed to set write end non-blocking");

    // Verify read end is non-blocking by attempting to read from empty pipe
    let mut buffer = [0u8; 1];
    let result = io::sys_read(read_fd, &mut buffer);
    assert!(result < 0);

    let err = std::io::Error::last_os_error();
    assert!(
        err.kind() == std::io::ErrorKind::WouldBlock,
        "Expected WouldBlock, got {:?}",
        err
    );

    // Cleanup
    io::sys_close(read_fd);
    io::sys_close(write_fd);
}

#[test]
fn test_safe_close_success() {
    // Create a pipe to get a valid fd
    let mut fds = [0i32; 2];
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    // Close using safe_close
    utils::safe_close(read_fd).expect("Failed to close read end");
    utils::safe_close(write_fd).expect("Failed to close write end");
}

#[test]
fn test_safe_close_invalid_fd() {
    // Closing an invalid fd should return an error
    let result = utils::safe_close(-1);
    assert!(result.is_err(), "Closing invalid fd should fail");
}

#[test]
fn test_sys_set_nonblocking_invalid_fd() {
    // Setting non-blocking on an invalid fd should fail
    let result = utils::sys_set_nonblocking(-1);
    assert!(
        result.is_err(),
        "Setting non-blocking on invalid fd should fail"
    );
}

#[test]
fn test_sys_set_nonblocking_socket() {
    use nucleus::socket;

    // Create a socket
    let fd = socket::sys_socket(libc::AF_INET).expect("Failed to create socket");

    // Socket should already be non-blocking (sys_socket does this)
    // But calling it again should succeed
    utils::sys_set_nonblocking(fd).expect("Failed to set non-blocking again");

    // Cleanup
    io::sys_close(fd);
}

#[test]
fn test_sys_set_nonblocking_on_file() {
    use std::ffi::CString;

    // Create a temporary file
    let path = CString::new("/tmp/nucleus_test_nonblock").unwrap();
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_CREAT | libc::O_RDWR, 0o644) };
    assert!(fd >= 0, "Failed to open file");

    // Set non-blocking
    utils::sys_set_nonblocking(fd).expect("Failed to set file non-blocking");

    // Cleanup
    io::sys_close(fd);
    unsafe { libc::unlink(path.as_ptr()) };
}

#[test]
fn test_close_twice_with_safe_close() {
    // Create a pipe
    let mut fds = [0i32; 2];
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
    }
    let fd = fds[0];
    io::sys_close(fds[1]);

    // First close should succeed
    utils::safe_close(fd).expect("First close should succeed");

    // Second close should fail
    let result = utils::safe_close(fd);
    assert!(result.is_err(), "Second close should fail");
}
