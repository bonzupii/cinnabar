//! The structured target descriptor and its static ABI tables.
//!
//! A `Target` is the single fact about which platform a build is for: the
//! operating system, the architecture, and the link mode, chosen once by
//! the CLI driver and carried on the codegen session. Every platform
//! variation the emitter needs — open and mmap flag values, error
//! accessors, socket entry points, descriptor widths, process ABI, and
//! Winsock initialization — is read from the typed `Abi` table keyed by
//! `TargetOs`, never by scraping the LLVM triple for a substring. The
//! native registry is universal; target differences select ABI data rather
//! than rejecting a language operation in the frontend.
//!
//! **Invariants:**
//! - `Target::triple()` is the only place an LLVM triple string is formed.
//! - `TargetOs::abi()` owns every numeric ABI constant the emitter reads.
//! - A named target defaults to `X86_64`; only `host` takes the compiler's
//!   own architecture and OS.

/// The operating system half of a target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetOs {
    Linux,
    Darwin,
    Bsd,
    Windows,
}

/// The architecture half of a target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetArch {
    X86_64,
    AArch64,
}

/// How a linked build is produced: shipped static musl, Windows MinGW,
/// or ordinary dynamic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetLink {
    Dynamic,
    StaticMusl,
    WindowsMinGW,
}

/// The native subsystems a declared verb can belong to; the resolver checks
/// each native verb's subsystem against the target before typechecking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeSubsystem {
    Core,
    Memory,
    File,
    Network,
    Process,
}

impl NativeSubsystem {
    /// The capability name a diagnostic reports when a target rejects it.
    pub fn name(self) -> &'static str {
        match self {
            NativeSubsystem::Core => "core operations",
            NativeSubsystem::Memory => "memory allocation",
            NativeSubsystem::File => "file access",
            NativeSubsystem::Network => "networking",
            NativeSubsystem::Process => "process management",
        }
    }
}

/// The complete description of one compilation target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Target {
    pub os: TargetOs,
    pub arch: TargetArch,
    pub link: TargetLink,
    host_toolchain: bool,
}

/// A rejected `--target` argument.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetError {
    /// The argument names no supported target.
    Unknown { argument: String },
}

impl std::fmt::Display for TargetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetError::Unknown { argument } => write!(
                f,
                "unknown target '{}'; choose host, an OS name, or an architecture-prefixed target",
                argument
            ),
        }
    }
}

/// The operating system this compiler binary itself runs on.
pub fn host_os() -> TargetOs {
    if cfg!(target_os = "macos") {
        TargetOs::Darwin
    } else if cfg!(target_os = "windows") {
        TargetOs::Windows
    } else if cfg!(target_os = "freebsd")
        || cfg!(target_os = "openbsd")
        || cfg!(target_os = "netbsd")
        || cfg!(target_os = "dragonfly")
    {
        TargetOs::Bsd
    } else {
        TargetOs::Linux
    }
}

/// The architecture this compiler binary itself runs on.
pub fn host_arch() -> TargetArch {
    if cfg!(target_arch = "aarch64") {
        TargetArch::AArch64
    } else {
        TargetArch::X86_64
    }
}

impl Target {
    /// The compiler's own platform, resolved at build time.
    pub fn host() -> Target {
        Target {
            os: host_os(),
            arch: host_arch(),
            link: TargetLink::Dynamic,
            host_toolchain: true,
        }
    }

    /// Parses a `--target` argument: `host`, a bare OS (default `X86_64`),
    /// or an explicit architecture before an OS name or triple.
    pub fn parse(name: &str) -> Result<Target, TargetError> {
        if name == "host" {
            return Ok(Target::host());
        }
        let explicit_arch = if name.starts_with("aarch64-") {
            Some(TargetArch::AArch64)
        } else if name.starts_with("x86_64-") {
            Some(TargetArch::X86_64)
        } else {
            None
        };
        let os_text = if let Some(rest) = name.strip_prefix("aarch64-") {
            rest
        } else if let Some(rest) = name.strip_prefix("x86_64-") {
            rest
        } else {
            name
        };
        let os = if os_text == "linux" || os_text.contains("linux") {
            TargetOs::Linux
        } else if os_text == "darwin" || os_text.contains("darwin") {
            TargetOs::Darwin
        } else if os_text == "bsd" || os_text.contains("freebsd") || os_text.contains("openbsd") || os_text.contains("netbsd") {
            TargetOs::Bsd
        } else if os_text == "windows" || os_text.contains("windows") {
            TargetOs::Windows
        } else {
            return Err(TargetError::Unknown { argument: name.to_string() });
        };
        let arch = match explicit_arch {
            Some(value) => value,
            None => TargetArch::X86_64
        };
        Ok(Target { os, arch, link: TargetLink::Dynamic, host_toolchain: false })
    }

    /// The LLVM triple for this target, the one place a triple is formed.
    pub fn triple(&self) -> String {
        let arch = match self.arch {
            TargetArch::X86_64 => "x86_64",
            TargetArch::AArch64 => "aarch64",
        };
        let os = match self.os {
            TargetOs::Linux => "unknown-linux-gnu",
            TargetOs::Darwin => "apple-darwin",
            TargetOs::Bsd => "unknown-freebsd",
            TargetOs::Windows => "w64-windows-gnu",
        };
        format!("{}-{}", arch, os)
    }

    /// True when this is the compiler's own OS and architecture.
    pub fn is_host(&self) -> bool {
        self.host_toolchain
    }

    pub fn supports_static_musl(&self) -> bool {
        self.os == TargetOs::Linux
    }

    pub fn link_mode(&self, static_link: bool) -> TargetLink {
        if static_link {
            TargetLink::StaticMusl
        } else if self.os == TargetOs::Windows {
            TargetLink::WindowsMinGW
        } else {
            TargetLink::Dynamic
        }
    }

    /// The typed ABI table for this target's operating system.
    pub fn abi(&self) -> Abi {
        self.os.abi()
    }

    /// The pointer width in bits of this target's architecture, the width
    /// `Isize`/`Usize` scalar literals are range-checked against.
    pub fn pointer_width_bits(&self) -> u32 {
        match self.arch {
            TargetArch::X86_64 => 64,
            TargetArch::AArch64 => 64,
        }
    }

    /// Whether a native subsystem is available: file/network require the
    /// target's ABI row to name an entry point.
    pub fn supports_subsystem(&self, subsystem: NativeSubsystem) -> bool {
        match subsystem {
            NativeSubsystem::Core | NativeSubsystem::Memory | NativeSubsystem::Process => true,
            NativeSubsystem::File => !self.abi().file_open.is_empty(),
            NativeSubsystem::Network => !self.abi().socket_create.is_empty(),
        }
    }
}

/// Static ABI facts for one operating system: the constants the emitter
/// lowers native calls against, plus errno and descriptor widths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Abi {
    /// `PROT_READ | PROT_WRITE` passed to `mmap`.
    pub prot_read_write: u64,
    /// `MAP_PRIVATE | MAP_ANONYMOUS` passed to `mmap`.
    pub map_private_anonymous: u64,
    /// `O_WRONLY` folded into `open` flags.
    pub open_write: u64,
    /// `O_CREAT` folded into `open` flags.
    pub open_create: u64,
    /// `O_TRUNC` folded into `open` flags.
    pub open_truncate: u64,
    /// `O_APPEND` folded into `open` flags.
    pub open_append: u64,
    /// `O_BINARY` folded into `open` flags (0 on POSIX).
    pub open_binary: u64,
    /// Whether anonymous private mappings are the memory allocator.
    pub memory_uses_mapping: bool,
    /// The mapping entry point selected by the memory ABI.
    pub memory_map: &'static str,
    /// The release entry point selected by the memory ABI.
    pub memory_release: &'static str,
    /// The file open entry point selected by the file ABI.
    pub file_open: &'static str,
    /// The file close entry point selected by the file ABI.
    pub file_close: &'static str,
    /// `ENAMETOOLONG`, the code a path longer than the kernel accepts
    /// reports.
    pub name_too_long: u64,
    /// `EINTR`, the code a syscall reports when a signal interrupted it.
    pub interrupted: u64,
    /// The C function returning the address of `errno`.
    pub errno_accessor: &'static str,
    /// The socket creation entry point.
    pub socket_create: &'static str,
    /// The socket bind entry point.
    pub socket_bind: &'static str,
    /// The socket listen entry point.
    pub socket_listen: &'static str,
    /// The socket accept entry point.
    pub socket_accept: &'static str,
    /// The socket send entry point.
    pub socket_send: &'static str,
    /// The socket receive entry point.
    pub socket_recv: &'static str,
    /// The C function used to retrieve a socket error.
    pub socket_error_accessor: &'static str,
    /// True when the socket error accessor returns a value rather than an
    /// address to an errno cell.
    pub socket_error_is_value: bool,
    /// The C function used to release a socket handle.
    pub socket_close: &'static str,
    /// True when the socket close function takes a 64-bit handle.
    pub socket_close_is_64: bool,
    /// True when the target process ABI uses Win32 process handles.
    pub process_is_windows: bool,
    /// True when dynamic linking accepts the non-PIE flag.
    pub supports_non_pie: bool,
    /// True when a socket descriptor is a 64-bit Winsock `SOCKET` rather
    /// than a 32-bit `int` file descriptor.
    pub socket_handle_is_64: bool,
    /// True when `read`/`write` return a 32-bit `int` rather than a 64-bit
    /// `ssize_t`.
    pub io_result_is_32: bool,
    /// True when the socket surface must call `WSAStartup` before first use.
    pub needs_winsock_init: bool,
}

impl TargetOs {
    /// The ABI row for this operating system.
    pub fn abi(self) -> Abi {
        match self {
            TargetOs::Linux => Abi {
                prot_read_write: 3,
                map_private_anonymous: 0x22,
                open_write: 1,
                open_create: 0o100,
                open_truncate: 0o1000,
                open_append: 0o2000,
                open_binary: 0,
                memory_uses_mapping: true,
                memory_map: "mmap",
                memory_release: "munmap",
                file_open: "open",
                file_close: "close",
                name_too_long: 36,
                interrupted: 4,
                errno_accessor: "__errno_location",
                socket_create: "socket",
                socket_bind: "bind",
                socket_listen: "listen",
                socket_accept: "accept",
                socket_send: "send",
                socket_recv: "recv",
                socket_error_accessor: "__errno_location",
                socket_error_is_value: false,
                socket_close: "close",
                socket_close_is_64: false,
                process_is_windows: false,
                supports_non_pie: true,
                socket_handle_is_64: false,
                io_result_is_32: false,
                needs_winsock_init: false,
            },
            TargetOs::Darwin => Abi {
                prot_read_write: 3,
                map_private_anonymous: 0x1002,
                open_write: 1,
                open_create: 0x200,
                open_truncate: 0x400,
                open_append: 0x8,
                open_binary: 0,
                memory_uses_mapping: true,
                memory_map: "mmap",
                memory_release: "munmap",
                file_open: "open",
                file_close: "close",
                name_too_long: 63,
                interrupted: 4,
                errno_accessor: "__error",
                socket_create: "socket",
                socket_bind: "bind",
                socket_listen: "listen",
                socket_accept: "accept",
                socket_send: "send",
                socket_recv: "recv",
                socket_error_accessor: "__error",
                socket_error_is_value: false,
                socket_close: "close",
                socket_close_is_64: false,
                process_is_windows: false,
                supports_non_pie: false,
                socket_handle_is_64: false,
                io_result_is_32: false,
                needs_winsock_init: false,
            },
            TargetOs::Bsd => Abi {
                prot_read_write: 3,
                map_private_anonymous: 0x1002,
                open_write: 1,
                open_create: 0x200,
                open_truncate: 0x400,
                open_append: 0x8,
                open_binary: 0,
                memory_uses_mapping: true,
                memory_map: "mmap",
                memory_release: "munmap",
                file_open: "open",
                file_close: "close",
                name_too_long: 63,
                interrupted: 4,
                errno_accessor: "__error",
                socket_create: "socket",
                socket_bind: "bind",
                socket_listen: "listen",
                socket_accept: "accept",
                socket_send: "send",
                socket_recv: "recv",
                socket_error_accessor: "__error",
                socket_error_is_value: false,
                socket_close: "close",
                socket_close_is_64: false,
                process_is_windows: false,
                supports_non_pie: true,
                socket_handle_is_64: false,
                io_result_is_32: false,
                needs_winsock_init: false,
            },
            TargetOs::Windows => Abi {
                prot_read_write: 0,
                map_private_anonymous: 0,
                open_write: 1,
                open_create: 0x100,
                open_truncate: 0x200,
                open_append: 0x8,
                open_binary: 0x8000,
                memory_uses_mapping: false,
                memory_map: "",
                memory_release: "free",
                file_open: "open",
                file_close: "_close",
                name_too_long: 36,
                interrupted: 4,
                errno_accessor: "_errno",
                socket_create: "socket",
                socket_bind: "bind",
                socket_listen: "listen",
                socket_accept: "accept",
                socket_send: "send",
                socket_recv: "recv",
                socket_error_accessor: "WSAGetLastError",
                socket_error_is_value: true,
                socket_close: "closesocket",
                socket_close_is_64: true,
                process_is_windows: true,
                supports_non_pie: false,
                socket_handle_is_64: true,
                io_result_is_32: true,
                needs_winsock_init: true,
            },
        }
    }

    /// The lowercase name of this operating system, for diagnostics.
    pub fn name(self) -> &'static str {
        match self {
            TargetOs::Linux => "linux",
            TargetOs::Darwin => "darwin",
            TargetOs::Bsd => "bsd",
            TargetOs::Windows => "windows",
        }
    }

}
