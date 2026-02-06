//! I/O module tests.
//!
//! Tests for low-level file descriptor I/O operations:
//! - `sys_read`
//! - `sys_write`
//! - `sys_close`

use nucleus::{io, utils};

#[test]
fn test_sys_read_from_pipe() {
    // Create a pipe for testing
    let mut fds = [0i32; 2];
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    // Write some data to the pipe
    let data = b"Hello, Nucleus!";
    let written = unsafe { libc::write(write_fd, data.as_ptr() as *const _, data.len()) };
    assert_eq!(written as usize, data.len());

    // Set read end to non-blocking
    utils::sys_set_nonblocking(read_fd).expect("Failed to set non-blocking");

    // Read the data back using sys_read
    let mut buffer = [0u8; 64];
    let bytes_read = io::sys_read(read_fd, &mut buffer);
    assert!(bytes_read > 0);
    assert_eq!(&buffer[..bytes_read as usize], data);

    // Cleanup
    io::sys_close(read_fd);
    io::sys_close(write_fd);
}

#[test]
fn test_sys_write_to_pipe() {
    // Create a pipe for testing
    let mut fds = [0i32; 2];
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    // Set write end to non-blocking
    utils::sys_set_nonblocking(write_fd).expect("Failed to set non-blocking");

    // Write data using sys_write
    let data = b"Test data for sys_write";
    let bytes_written = io::sys_write(write_fd, data);
    assert_eq!(bytes_written as usize, data.len());

    // Read back to verify
    let mut buffer = [0u8; 64];
    let bytes_read = unsafe { libc::read(read_fd, buffer.as_mut_ptr() as *mut _, buffer.len()) };
    assert_eq!(bytes_read as usize, data.len());
    assert_eq!(&buffer[..data.len()], data);

    // Cleanup
    io::sys_close(read_fd);
    io::sys_close(write_fd);
}

#[test]
fn test_sys_read_empty_pipe_returns_wouldblock() {
    // Create a pipe
    let mut fds = [0i32; 2];
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    // Set read end to non-blocking
    utils::sys_set_nonblocking(read_fd).expect("Failed to set non-blocking");

    // Try to read from empty pipe - should return -1 with EAGAIN
    let mut buffer = [0u8; 64];
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
fn test_sys_close_pipe() {
    // Create a pipe
    let mut fds = [0i32; 2];
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    // Close both ends
    io::sys_close(read_fd);
    io::sys_close(write_fd);

    // Verify they're closed by trying to read (should fail)
    let mut buffer = [0u8; 1];
    let result = io::sys_read(read_fd, &mut buffer);
    assert!(result < 0);
}

#[test]
fn test_sys_read_write_large_data() {
    // Create a pipe
    let mut fds = [0i32; 2];
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    // Set non-blocking
    utils::sys_set_nonblocking(read_fd).expect("Failed to set non-blocking");
    utils::sys_set_nonblocking(write_fd).expect("Failed to set non-blocking");

    // Write data
    let data: Vec<u8> = (0u8..255).cycle().take(4096).collect();
    let mut total_written = 0;

    // Write as much as possible
    while total_written < data.len() {
        let written = io::sys_write(write_fd, &data[total_written..]);
        if written > 0 {
            total_written += written as usize;
        } else {
            break;
        }
    }

    // Read it all back
    let mut buffer = vec![0u8; total_written];
    let mut total_read = 0;

    while total_read < total_written {
        let read = io::sys_read(read_fd, &mut buffer[total_read..]);
        if read > 0 {
            total_read += read as usize;
        } else {
            break;
        }
    }

    assert_eq!(total_read, total_written);
    assert_eq!(&buffer[..total_read], &data[..total_written]);

    // Cleanup
    io::sys_close(read_fd);
    io::sys_close(write_fd);
}
