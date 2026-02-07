//! Filesystem module tests.
//!
//! Tests for filesystem operations:
//! - `sys_open`
//! - `sys_mkdir`
//! - Constants: `OPENFLAGS`, `CREATEFLAGS`

use nucleus::{fs, io};
use std::ffi::CString;

/// Helper to check if an fd is valid (cross-platform).
/// On Unix, invalid fd is -1; on Windows, it's u64::MAX.
#[cfg(unix)]
fn is_valid_fd(fd: io::RawFd) -> bool {
    fd >= 0
}

#[cfg(windows)]
fn is_valid_fd(fd: io::RawFd) -> bool {
    fd != u64::MAX
}

/// Get a temporary file path as CString.
fn temp_path(name: &str) -> CString {
    let mut path = std::env::temp_dir();
    path.push(name);
    CString::new(path.to_str().unwrap()).unwrap()
}

/// Cleanup helper - removes a file if it exists.
fn cleanup_file(path: &CString) {
    let path_str = path.to_str().unwrap();
    let _ = std::fs::remove_file(path_str);
}

/// Cleanup helper - removes a directory if it exists.
fn cleanup_dir(path: &CString) {
    let path_str = path.to_str().unwrap();
    let _ = std::fs::remove_dir(path_str);
}

#[test]
fn test_sys_open_create_file() {
    let path = temp_path("nucleus_test_create");

    // Clean up any existing file
    cleanup_file(&path);

    // Create a new file
    let fd = unsafe { fs::sys_open(path.as_ptr(), fs::CREATEFLAGS, 0o644) };
    assert!(is_valid_fd(fd), "Failed to create file, fd = {}", fd);

    // Write some data to verify it works
    let data = b"test data";
    let written = io::sys_write(fd, data);
    assert!(written > 0, "Failed to write to created file");

    // Cleanup
    io::sys_close(fd);
    cleanup_file(&path);
}

#[test]
fn test_sys_open_read_file() {
    let path = temp_path("nucleus_test_read");

    // Clean up first
    cleanup_file(&path);

    // Create the file first
    let create_fd = unsafe { fs::sys_open(path.as_ptr(), fs::CREATEFLAGS, 0o644) };
    assert!(is_valid_fd(create_fd), "Failed to create file");

    // Write some data
    let data = b"hello world";
    let written = io::sys_write(create_fd, data);
    assert_eq!(written as usize, data.len());
    io::sys_close(create_fd);

    // Open for reading
    let read_fd = unsafe { fs::sys_open(path.as_ptr(), fs::OPENFLAGS, 0) };
    assert!(is_valid_fd(read_fd), "Failed to open file for reading");

    // Read the data back
    let mut buffer = [0u8; 64];
    let read = io::sys_read(read_fd, &mut buffer);
    assert!(read > 0, "Failed to read from file");
    assert_eq!(&buffer[..read as usize], data);

    // Cleanup
    io::sys_close(read_fd);
    cleanup_file(&path);
}

#[test]
fn test_sys_open_nonexistent_file() {
    let path = temp_path("nucleus_nonexistent_file_12345");

    // Ensure it doesn't exist
    cleanup_file(&path);

    // Try to open non-existent file with OPENFLAGS (should fail)
    let fd = unsafe { fs::sys_open(path.as_ptr(), fs::OPENFLAGS, 0) };
    assert!(!is_valid_fd(fd), "Opening non-existent file should fail");
}

#[test]
fn test_sys_mkdir() {
    let path = temp_path("nucleus_test_dir");

    // Remove if exists
    cleanup_dir(&path);

    // Create directory
    let result = unsafe { fs::sys_mkdir(path.as_ptr(), 0o755) };
    assert_eq!(result, 0, "Failed to create directory");

    // Verify it exists by trying to create again (should fail)
    let result2 = unsafe { fs::sys_mkdir(path.as_ptr(), 0o755) };
    assert!(result2 != 0, "Creating existing directory should fail");

    // Cleanup
    cleanup_dir(&path);
}

#[test]
fn test_sys_mkdir_nested_fails() {
    let mut nested = std::env::temp_dir();
    nested.push("nucleus_nested_not_exist");
    nested.push("subdir");
    nested.push("deep");
    let path = CString::new(nested.to_str().unwrap()).unwrap();

    // Clean up parent if it somehow exists
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("nucleus_nested_not_exist"));

    // Creating nested directories should fail (mkdir doesn't create parents)
    let result = unsafe { fs::sys_mkdir(path.as_ptr(), 0o755) };
    assert!(
        result != 0,
        "Creating nested dirs without parents should fail"
    );
}

// Unix-only test: checks POSIX permission bits
#[cfg(unix)]
#[test]
fn test_sys_open_with_different_modes() {
    let path = temp_path("nucleus_test_modes");

    // Clean up
    cleanup_file(&path);

    // Create with restrictive permissions
    let fd = unsafe { fs::sys_open(path.as_ptr(), fs::CREATEFLAGS, 0o600) };
    assert!(is_valid_fd(fd), "Failed to create file with mode 0o600");
    io::sys_close(fd);

    // Verify permissions
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    let stat_result = unsafe { libc::stat(path.as_ptr(), &mut stat) };
    assert_eq!(stat_result, 0, "Failed to stat file");

    // Check mode (mask with 0o777 to get just permission bits)
    let mode = stat.st_mode & 0o777;
    assert_eq!(mode, 0o600, "File mode should be 0o600, got {:o}", mode);

    // Cleanup
    cleanup_file(&path);
}

#[test]
fn test_sys_open_truncates_existing() {
    let path = temp_path("nucleus_test_trunc");

    // Clean up first
    cleanup_file(&path);

    // Create file with some content
    let fd1 = unsafe { fs::sys_open(path.as_ptr(), fs::CREATEFLAGS, 0o644) };
    assert!(is_valid_fd(fd1));
    let data = b"original content that is long";
    io::sys_write(fd1, data);
    io::sys_close(fd1);

    // Reopen with CREATEFLAGS (should truncate)
    let fd2 = unsafe { fs::sys_open(path.as_ptr(), fs::CREATEFLAGS, 0o644) };
    assert!(is_valid_fd(fd2));

    // Write shorter content
    let short_data = b"short";
    io::sys_write(fd2, short_data);
    io::sys_close(fd2);

    // Read back and verify truncation
    let fd3 = unsafe { fs::sys_open(path.as_ptr(), fs::OPENFLAGS, 0) };
    assert!(is_valid_fd(fd3));

    let mut buffer = [0u8; 64];
    let read = io::sys_read(fd3, &mut buffer);
    assert_eq!(read as usize, short_data.len());
    assert_eq!(&buffer[..read as usize], short_data);

    io::sys_close(fd3);
    cleanup_file(&path);
}
