//! Direct Linux system calls, emitted as inline assembly.
//!
//! A Cinnabar binary links `-static -nostdlib` against an embedded musl, so
//! a libc *is* present — `Collections` uses its allocator, and `Net` uses
//! its socket wrappers, because an allocator and a `sockaddr` marshaller
//! are real code rather than thin syscall shims. The `Memory`, `Terminal`,
//! and `File` surfaces do not go through it. They issue the kernel entry
//! point directly, which is what Milestone 4 asks for and what makes those
//! three surfaces auditable end to end: the instruction that leaves user
//! space is visible in the emitted IR, with nothing between the Cinnabar
//! declaration and the kernel.
//!
//! Everything architecture-specific is in this file and derived from the
//! module's target triple: the instruction, the register constraints, and
//! the syscall numbers. Those numbers are irreducible per-ABI data — there
//! is nowhere to derive `SYS_write == 1 on x86_64, 64 on AArch64` *from* —
//! so they live in one table, keyed by an architecture and a logical
//! operation, rather than being spread across the call sites.
//!
//! ## Return convention
//!
//! A Linux syscall returns its result in the same register that carried the
//! first argument, and reports failure as a **negative errno** in that
//! register rather than through a separate `errno` variable. Callers test
//! the returned `i64` for `< 0` and negate it to recover the error code.
//! This is why the syscall path needs no `__errno_location` (which the libc
//! wrappers, and so the `Net` surface, still use): the error arrives in the
//! value itself.
//!
//! **Invariants:**
//! - An architecture with no implemented table is a compile error naming
//!   the triple, never a guessed syscall number. A wrong number here would
//!   not fail to build; it would silently call something else.
//! - The register constraints are part of the ABI data, not incidental:
//!   x86_64 passes the fourth argument in `r10` rather than `rcx` because
//!   `syscall` overwrites `rcx`. `openat` is used on both architectures
//!   because AArch64 has no `open`.

use crate::codegen::error::*;
use inkwell::context::Context;
use inkwell::types::BasicMetadataTypeEnum;
use inkwell::values::{BasicMetadataValueEnum, IntValue, ValueKind};
use inkwell::InlineAsmDialect;

/// The architectures whose Linux syscall ABI is implemented.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    AArch64,
}

/// A kernel entry point, named for what it does rather than for any libc
/// function of the same name.
///
/// `OpenAt` rather than `open` on purpose: AArch64's Linux ABI has no
/// `open` at all, only `openat`, so using `openat` everywhere (with
/// `AT_FDCWD` as the directory) keeps one code path instead of an
/// architecture-conditional file surface.
#[derive(Clone, Copy)]
pub enum Sys {
    Read,
    Write,
    OpenAt,
    Close,
    Mmap,
    Munmap,
}

/// `openat`'s "relative to the current working directory" sentinel.
pub const AT_FDCWD: i64 = -100;

/// `mmap` protection bits: readable and writable.
pub const PROT_READ_WRITE: i64 = 0x1 | 0x2;

/// `mmap` flags: a private anonymous mapping — memory backed by no file,
/// not shared with other processes.
pub const MAP_PRIVATE_ANONYMOUS: i64 = 0x02 | 0x20;

/// `openat` flags. Read-only, write-only, and the create/truncate/append
/// modifiers a write-mode open needs. These are the Linux values and are
/// the same on x86_64 and AArch64.
pub const O_RDONLY: i64 = 0;
pub const O_WRONLY: i64 = 1;
pub const O_CREAT: i64 = 0o100;
pub const O_TRUNC: i64 = 0o1000;
pub const O_APPEND: i64 = 0o2000;

/// Mode bits for a file this program creates: `rw-r--r--`.
pub const CREATE_MODE: i64 = 0o644;

/// The architecture a target triple names, or `None` when its syscall ABI
/// is not implemented.
///
/// Returning `None` rather than guessing is the point: a syscall number is
/// meaningless on an architecture whose table is absent, and emitting one
/// anyway would produce a binary that calls an arbitrary kernel entry
/// point. The caller turns this into a compile error naming the triple.
pub fn arch_of(triple: &str) -> Option<Arch> {
    let name = triple.split('-').next()?;
    if name == "x86_64" {
        Some(Arch::X86_64)
    } else if name == "aarch64" {
        Some(Arch::AArch64)
    } else {
        None
    }
}

/// The Linux syscall number for an operation on an architecture.
///
/// This table is the irreducible ABI data: the numbers are assigned by the
/// kernel per architecture and cannot be derived from anything else in the
/// compiler. Keeping them in a single exhaustive match — rather than
/// scattered at the call sites — is what makes adding an architecture a
/// change in one place.
pub fn number(arch: Arch, call: Sys) -> u64 {
    match arch {
        Arch::X86_64 => match call {
            Sys::Read => 0,
            Sys::Write => 1,
            Sys::Close => 3,
            Sys::Mmap => 9,
            Sys::Munmap => 11,
            Sys::OpenAt => 257,
        },
        Arch::AArch64 => match call {
            Sys::OpenAt => 56,
            Sys::Close => 57,
            Sys::Read => 63,
            Sys::Write => 64,
            Sys::Munmap => 215,
            Sys::Mmap => 222,
        },
    }
}

/// The registers a syscall's arguments travel in, in order, and the
/// register the number itself travels in.
///
/// x86_64 takes the number in `rax` and arguments in `rdi, rsi, rdx, r10,
/// r8, r9` — note `r10`, not `rcx`, because the `syscall` instruction
/// destroys `rcx` and `r11`. AArch64 takes the number in `x8` and
/// arguments in `x0..x5`, and returns in `x0`, which is also the first
/// argument register.
fn registers(arch: Arch) -> (&'static str, [&'static str; 6]) {
    match arch {
        Arch::X86_64 => ("rax", ["rdi", "rsi", "rdx", "r10", "r8", "r9"]),
        Arch::AArch64 => ("x8", ["x0", "x1", "x2", "x3", "x4", "x5"]),
    }
}

/// The instruction that enters the kernel, and the registers it destroys.
///
/// `~{memory}` is on both: a syscall can read or write any memory the
/// process owns (`read` fills a buffer, `write` reads one), so the
/// optimizer must not move loads and stores across it or cache values it
/// may have changed.
fn instruction(arch: Arch) -> (&'static str, &'static str) {
    match arch {
        // `syscall` clobbers rcx (with the return address) and r11 (with
        // rflags); `~{dirflag},~{fpsr},~{flags}` is what LLVM expects for
        // any x86 asm that touches condition flags.
        Arch::X86_64 => ("syscall", ",~{rcx},~{r11},~{memory},~{dirflag},~{fpsr},~{flags}"),
        Arch::AArch64 => ("svc #0", ",~{memory}"),
    }
}

/// The result register's constraint — where the kernel leaves the return
/// value.
fn result_register(arch: Arch) -> &'static str {
    match arch {
        Arch::X86_64 => "rax",
        Arch::AArch64 => "x0",
    }
}

/// Emits one system call and returns its raw `i64` result: the value on
/// success, or a negative errno on failure.
///
/// All arguments are passed as `i64`. A syscall argument register is a full
/// machine word whatever the C prototype says, so widening a file
/// descriptor or a length to `i64` at the call site is the ABI, not a
/// convenience — this is the Milestone 1 dependency the roadmap names.
pub fn emit<'ctx>(
    context: &'ctx Context,
    builder: &inkwell::builder::Builder<'ctx>,
    arch: Arch,
    call: Sys,
    args: &[IntValue<'ctx>],
    span: (i64, i64, i64),
) -> Result<IntValue<'ctx>, CodegenError> {
    let (number_reg, arg_regs) = registers(arch);
    if args.len() > arg_regs.len() {
        return Err(builder_error(
            span.0,
            span.1,
            span.2,
            &format!("internal: system call takes {} arguments, more than the {} the ABI passes in registers", args.len(), arg_regs.len()),
        ));
    }
    let i64_ty = context.i64_type();
    // The number first, then the arguments: the operand order here must
    // match the constraint string built below, since LLVM binds inline-asm
    // operands positionally.
    let mut operand_tys: Vec<BasicMetadataTypeEnum> = vec![i64_ty.into()];
    let mut operands: Vec<BasicMetadataValueEnum> = vec![i64_ty.const_int(number(arch, call), false).into()];
    let mut constraints = format!("={{{}}},{{{}}}", result_register(arch), number_reg);
    let mut idx = 0usize;
    while idx < args.len() {
        let arg = match args.get(idx) {
            Some(value) => *value,
            None => break,
        };
        let reg = match arg_regs.get(idx) {
            Some(name) => *name,
            None => break,
        };
        operand_tys.push(i64_ty.into());
        operands.push(arg.into());
        constraints.push_str(&format!(",{{{}}}", reg));
        idx += 1;
    }
    let (text, clobbers) = instruction(arch);
    constraints.push_str(clobbers);
    let asm_ty = i64_ty.fn_type(&operand_tys, false);
    // `sideeffect` because a syscall's effect is not its return value: a
    // `write` whose result is discarded must still happen, so the call may
    // never be deleted as dead.
    let asm = context.create_inline_asm(
        asm_ty,
        text.to_string(),
        constraints,
        true,
        false,
        Some(InlineAsmDialect::ATT),
        false,
    );
    let call_site = builder
        .build_indirect_call(asm_ty, asm, &operands, "")
        .map_err(|err| builder_error(span.0, span.1, span.2, &format!("internal: cannot emit a system call: {}", err)))?;
    match call_site.try_as_basic_value() {
        ValueKind::Basic(value) => Ok(value.into_int_value()),
        ValueKind::Instruction(inst) => Err(builder_error(
            span.0,
            span.1,
            span.2,
            &format!("internal: system call produced no value ({:?})", inst.get_opcode()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The Linux syscall numbers, written out independently of the table
    // above from the kernel's own architecture syscall lists
    // (`arch/x86/entry/syscalls/syscall_64.tbl` and the generic
    // `include/uapi/asm-generic/unistd.h` that AArch64 uses). A wrong
    // number here is not a crash but a *different syscall*, which is why
    // the check is a second hand-written copy rather than a re-read of the
    // same data: two independent statements of the same fact that must
    // agree.
    #[test]
    fn x86_64_syscall_numbers_match_the_linux_abi() {
        assert_eq!(number(Arch::X86_64, Sys::Read), 0);
        assert_eq!(number(Arch::X86_64, Sys::Write), 1);
        assert_eq!(number(Arch::X86_64, Sys::Close), 3);
        assert_eq!(number(Arch::X86_64, Sys::Mmap), 9);
        assert_eq!(number(Arch::X86_64, Sys::Munmap), 11);
        assert_eq!(number(Arch::X86_64, Sys::OpenAt), 257);
    }

    #[test]
    fn aarch64_syscall_numbers_match_the_generic_abi() {
        assert_eq!(number(Arch::AArch64, Sys::OpenAt), 56);
        assert_eq!(number(Arch::AArch64, Sys::Close), 57);
        assert_eq!(number(Arch::AArch64, Sys::Read), 63);
        assert_eq!(number(Arch::AArch64, Sys::Write), 64);
        assert_eq!(number(Arch::AArch64, Sys::Munmap), 215);
        assert_eq!(number(Arch::AArch64, Sys::Mmap), 222);
    }

    // Every operation must have a distinct number within an architecture:
    // a duplicated entry would silently route one surface's calls to
    // another's kernel entry point.
    #[test]
    fn numbers_are_distinct_within_an_architecture() {
        let calls = [Sys::Read, Sys::Write, Sys::OpenAt, Sys::Close, Sys::Mmap, Sys::Munmap];
        for arch in [Arch::X86_64, Arch::AArch64] {
            let mut seen: Vec<u64> = Vec::new();
            for call in calls {
                let value = number(arch, call);
                assert!(!seen.contains(&value), "duplicate syscall number {}", value);
                seen.push(value);
            }
        }
    }

    #[test]
    fn recognizes_the_implemented_architectures() {
        assert!(arch_of("x86_64-unknown-linux-musl").is_some());
        assert!(arch_of("aarch64-unknown-linux-musl").is_some());
    }

    // An unimplemented architecture must report as unknown rather than
    // fall back to one of the two tables: a syscall number is meaningless
    // on an architecture it was not assigned for, and guessing would emit
    // a binary that calls an arbitrary kernel entry point.
    #[test]
    fn refuses_to_guess_an_unimplemented_architecture() {
        assert!(arch_of("riscv64-unknown-linux-gnu").is_none());
        assert!(arch_of("armv7-unknown-linux-gnueabihf").is_none());
        assert!(arch_of("").is_none());
    }

    // x86_64 passes the fourth argument in r10, not rcx: the `syscall`
    // instruction overwrites rcx with the return address, so a table that
    // used the C calling convention's rcx would corrupt every 4-argument
    // call. `mmap` takes six arguments and is the reason this matters.
    #[test]
    fn x86_64_uses_r10_for_the_fourth_argument() {
        let (number_reg, arg_regs) = registers(Arch::X86_64);
        assert_eq!(number_reg, "rax");
        assert_eq!(arg_regs, ["rdi", "rsi", "rdx", "r10", "r8", "r9"]);
    }

    #[test]
    fn aarch64_takes_the_number_in_x8() {
        let (number_reg, arg_regs) = registers(Arch::AArch64);
        assert_eq!(number_reg, "x8");
        assert_eq!(arg_regs, ["x0", "x1", "x2", "x3", "x4", "x5"]);
    }

    // Both architectures must declare a memory clobber. A syscall can read
    // or write any memory the process owns -- `read` fills a buffer,
    // `write` reads one -- so without it the optimizer may move loads and
    // stores across the call or cache values it changed.
    #[test]
    fn every_architecture_clobbers_memory() {
        for arch in [Arch::X86_64, Arch::AArch64] {
            let (text, clobbers) = instruction(arch);
            assert!(!text.is_empty());
            assert!(clobbers.contains("~{memory}"), "{} does not clobber memory", text);
        }
    }
}
