# Security Policy

Cinnabar is a compiler, not a network service, so "security" here mostly means **soundness**: a program the compiler accepts must behave the way its declared types and borrow rules promise. A bug that lets unsafe or undefined behavior through as "safe, checked" code is a security issue, not just a correctness bug. In scope:

- The borrow checker accepting a use-after-move, a double-free, or an aliasing `&mut` violation.
- The type checker or codegen producing memory-unsafe machine code from a program that should have been rejected, or miscompiling a program that should have been accepted, in a way that leads to memory corruption.
- Memory-safety issues in the compiler's own handling of untrusted input (a crafted `.cnb` file, project file, or CLI argument causing a crash, out-of-bounds read, or worse in the compiler binary itself).
- Vulnerabilities in a pinned dependency that are reachable through the compiler or its build.

Not in scope: the compiler rejecting a valid program with a wrong-but-safe diagnostic, or crashing with a Rust panic on malformed input — file those as regular [GitHub Issues](https://github.com/bonzupii/cinnabar/issues) instead (a panic still violates project conventions per [`AGENTS.md`](AGENTS.md), but it isn't a vulnerability unless it's demonstrably exploitable).

## Reporting a vulnerability

Please **do not** open a public issue for a security-relevant bug. Instead, use GitHub's private reporting:

1. Go to the [Security tab](https://github.com/bonzupii/cinnabar/security) of this repository.
2. Click **"Report a vulnerability"** to open a private advisory.

If private reporting isn't available for any reason, email the maintainer directly (see the GitHub profile at [bonzupii](https://github.com/bonzupii)) with `[SECURITY] cinnabar` in the subject line.

Include a minimal reproduction (ideally a `.cnb` fixture), the observed behavior, and why it's unsound or unsafe. Expect an initial response within a week; the project is under active early development by a small team, so triage and fix timelines vary with severity.

## Supported versions

Cinnabar is pre-1.0 and does not yet maintain multiple supported release branches. Fixes land on `main`; there is no backport policy until a first stable release exists.
