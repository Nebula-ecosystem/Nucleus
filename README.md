# ⚙️ Nucleus

[![License](https://img.shields.io/badge/license-SSPL-blue.svg)](LICENSE)
![Dev Rust](https://img.shields.io/badge/Developed%20with-Rust%201.92.0-orange)
[![CI](https://github.com/Nebula-ecosystem/Nucleus/actions/workflows/ci.yml/badge.svg)](https://github.com/Nebula-ecosystem/Nucleus/actions/workflows/ci.yml)

**Nucleus** is the low-level system crate of the ***Nebula*** ecosystem, providing a thin, cross-platform abstraction over OS system calls.

---

## 📊 Project Status

- [x] **Event Polling**
  - [x] epoll (Linux)
  - [x] kqueue (BSD / macOS)
  - [x] WSAPoll (Windows)

- [ ] **Filesystem**
  - [ ] Disk space information (free / total / available)
  - [ ] Cross-platform path handling (Unix / Windows)

---

## 🚀 Getting Started

This crate is not published on crates.io. Add it directly from GitHub:

``` toml
[dependencies]
nucleus = { git = "https://github.com/Nebula-ecosystem/Nucleus" }
```

---

## ⚠️ Safety Notice

Nucleus contains **unsafe code** and direct **system call bindings**.

- APIs aim to be minimal and explicit.
- Platform-specific behavior may differ subtly.
- Internal implementations may change as abstractions mature.

The crate is intended for **runtime development**, **systems programming**,  
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