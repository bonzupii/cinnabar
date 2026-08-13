// Locates musl's static libc.a (and the crt start objects that a fully
// static link requires for `_start`) and stages them into Cargo's OUT_DIR,
// where src/codegen/mod.rs embeds them with include_bytes!.  Nothing binary
// ever lives in the source tree; every archive is staged dynamically at
// build time from the discovery order below:
//   1. the MUSL_LIBC_A environment variable,
//   2. the nix store (pkgs.musl, as provisioned by the flake dev shell),
//   3. standard host musl paths,
//   4. the rustc sysroot.
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const STAGE_NAMES: &[&str] = &["libc.a", "crt1.o", "crti.o", "crtn.o"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let lib_dir = find_musl_lib_dir()?;
    let mut idx = 0usize;
    while idx < STAGE_NAMES.len() {
        match STAGE_NAMES.get(idx) {
            Some(name) => {
                // The rustc sysroot keeps libc.a and the crt start objects in a
                // `self-contained` subdirectory of the target lib dir; musl's
                // own layout keeps them at the top level.
                let mut from = lib_dir.join(name);
                if !from.is_file() {
                    from = lib_dir.join("self-contained").join(name);
                }
                let to = out_dir.join(name);
                // Stage via a temp file + atomic rename.  nix store paths are
                // read-only and fs::copy preserves those bits, so overwriting
                // a previously staged read-only file needs a directory
                // operation; rename replaces it regardless of its own bits.
                let bytes = fs::read(&from)?;
                let tmp = out_dir.join(format!("{}.tmp", name));
                fs::write(&tmp, bytes)?;
                fs::rename(&tmp, &to)?;
                println!("cargo:rerun-if-changed={}", from.display());
            }
            None => break,
        }
        idx += 1;
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=MUSL_LIBC_A");
    Ok(())
}
// The first directory (in discovery order) that holds a musl libc.a:
//   1. the MUSL_LIBC_A environment variable,
//   2. the nix store (pkgs.musl, as provisioned by the flake dev shell),
//   3. standard host musl paths,
//   4. the rustc sysroot.
fn find_musl_lib_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(path) = env::var("MUSL_LIBC_A") {
        let archive = PathBuf::from(&path);
        if archive.is_file()
            && let Some(dir) = archive.parent()
        {
            dirs.push(dir.to_path_buf());
        }
    }
    nix_store_dirs(&mut dirs);
    for path in [
        "/usr/lib/x86_64-linux-musl",
        "/usr/lib/aarch64-linux-musl",
        "/lib/x86_64-linux-musl",
        "/lib/aarch64-linux-musl",
    ] {
        dirs.push(PathBuf::from(path));
    }
    rustc_sysroot_dirs(&mut dirs);
    let mut idx = 0usize;
    while idx < dirs.len() {
        if let Some(dir) = dirs.get(idx)
            && holds_libc_a(dir)
        {
            return Ok(dir.clone());
        }
        idx += 1;
    }
    Err("no musl libc.a found: set MUSL_LIBC_A, install musl, or add a rust musl target".into())
}

// A directory counts as a musl lib dir when it holds libc.a either directly
// (musl's own layout, as installed by nix or a host package) or under the
// `self-contained` subdirectory that rustup uses for its musl targets.  The
// staging loop resolves each archive against the same two locations.
fn holds_libc_a(dir: &Path) -> bool {
    dir.join("libc.a").is_file() || dir.join("self-contained").join("libc.a").is_file()
}

// Every `/nix/store/<hash>-musl-*/lib` directory that contains libc.a.
fn nix_store_dirs(dirs: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir("/nix/store") {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("build.rs: cannot scan /nix/store: {}", err);
            return;
        }
    };
    for entry in entries {
        if let Ok(entry) = entry
            && entry.file_name().to_string_lossy().contains("-musl-")
        {
            let lib = entry.path().join("lib");
            if holds_libc_a(&lib) {
                dirs.push(lib);
            }
        }
    }
}

// `<rustc sysroot>/lib/rustlib/<musl-target>/lib` for every installed target.
fn rustc_sysroot_dirs(dirs: &mut Vec<PathBuf>) {
    let output = match Command::new("rustc").args(["--print", "sysroot"]).output() {
        Ok(output) => output,
        Err(err) => {
            eprintln!("build.rs: cannot query rustc sysroot: {}", err);
            return;
        }
    };
    if !output.status.success() {
        eprintln!(
            "build.rs: rustc --print sysroot failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return;
    }
    let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();
    for target in ["x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"] {
        let lib = Path::new(&sysroot)
            .join("lib")
            .join("rustlib")
            .join(target)
            .join("lib");
        if holds_libc_a(&lib) {
            dirs.push(lib);
        }
    }
}
