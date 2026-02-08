# ⚙️ Nucleus

[![License](https://img.shields.io/badge/license-SSPL-blue.svg)](LICENSE)
![Dev Rust](https://img.shields.io/badge/Developed%20with-Rust%201.92.0-orange)
[![CI](https://github.com/Nebula-ecosystem/Nucleus/actions/workflows/ci.yml/badge.svg)](https://github.com/Nebula-ecosystem/Nucleus/actions/workflows/ci.yml)

**Nucleus** is the low-level system crate of the ***Nebula*** ecosystem, providing a thin, cross-platform abstraction over OS system calls for the Cadentis async runtime.

---

## 📊 Project Status

- [x] **Event-Driven I/O Polling**
  - [x] epoll + eventfd (Linux)
  - [x] kqueue + EVFILT_USER (macOS)
  - [x] WSAPoll + UDP waker (Windows)

- [x] **Raw File Descriptor I/O**
  - [x] sys_read (non-blocking read)
  - [x] sys_write (non-blocking write)
  - [x] sys_close (descriptor cleanup)

- [x] **Filesystem Operations**
  - [x] sys_open (open files with POSIX flags)
  - [x] sys_mkdir (create directories)
  - [x] Default flag constants (OPENFLAGS, CREATEFLAGS)

- [x] **TCP Socket Lifecycle**
  - [x] sys_socket (create non-blocking TCP sockets)
  - [x] sys_bind (bind to local address)
  - [x] sys_listen (mark as passive)
  - [x] sys_accept (accept connections)
  - [x] sys_connect (initiate connections)
  - [x] sys_shutdown (teardown)
  - [x] Socket options (SO_REUSEADDR, IPV6_V6ONLY, SO_ERROR)

- [x] **Address Conversions**
  - [x] SocketAddr ↔ sockaddr_storage (IPv4 & IPv6)
  - [x] sys_parse_sockaddr (string → sockaddr_storage)
  - [x] sys_local_addr (retrieve bound address)

- [x] **Platform Utilities**
  - [x] sys_set_nonblocking (Unix)
  - [x] safe_close with error reporting (Unix)
  - [x] ensure_winsock initialization (Windows)
  - [x] is_socket detection (Windows)

- [x] **Advanced Filesystem**
  - [x] Disk space information (free / total / available)

---

## 🚀 Getting Started

This crate is not published on crates.io. Add it directly from GitHub:

``` toml
[dependencies]
nucleus = { git = "https://github.com/Nebula-ecosystem/Nucleus" }
```

---

## 📝 Example: TCP Server Socket

Create a non-blocking TCP listener on port 8080:

```rust
use nucleus::{sys_socket, sys_bind, sys_listen, sys_parse_sockaddr, AF_INET};

fn main() -> std::io::Result<()> {
    // Create a non-blocking TCP socket
    let fd = sys_socket(AF_INET)?;
    
    // Parse address and bind
    let (storage, len) = sys_parse_sockaddr("127.0.0.1:8080")?;
    sys_bind(fd, &storage, len)?;
    
    // Mark socket as listening
    sys_listen(fd)?;
    
    Ok(())
}
```

---

## ⚠️ Safety Notice

Nucleus contains **unsafe code** and direct **system call bindings**.

- Do **not** use in production environments without thorough testing.
- APIs aim to be minimal and explicit but may change.
- Platform-specific behavior may differ subtly across Unix and Windows.
- Thread safety guarantees are documented per-function.
- Internal implementations may change as abstractions mature.

The crate is designed primarily for **runtime development**, **systems programming**,  
and internal use within the Nebula ecosystem.

---

## 📖 Documentation

You can generate the full API documentation locally using Cargo:

``` bash
cargo doc --open
```

This will build and open the documentation for Nucleus and all its public APIs in your browser.

---

## 🦀 Rust Version

- **Developed with**: Rust 1.92.0
- **MSRV**: Rust 1.92.0 (may increase in the future)

---

## 📄 License Philosophy

Nucleus is licensed under the **Server Side Public License (SSPL) v1**.

This license is intentionally chosen to protect the integrity of the Nebula ecosystem.  
While the project is fully open for **contribution, improvement, and transparency**,  
SSPL prevents third parties from creating competing platforms, proprietary versions,  
or commercial services derived from the project.

Nebula is designed to grow as **one unified, community-driven network**.  
By using SSPL, we ensure that:

- all improvements remain open and benefit the ecosystem,  
- the system layer remains transparent and auditable,  
- the platform does not fragment into incompatible forks,  
- companies cannot exploit the project without contributing back.

In short, SSPL ensures that **Nucleus — the foundation of Nebula —**  
remains **open, stable, and protected from exploitation**.

---

## 🤝 Contact

For questions, discussions, or contributions, feel free to reach out:

- **Discord**: enzoblain
- **Email**: [enzoblain@proton.me](mailto:enzoblain@proton.me)