//! Staging of a static musl libc into `OUT_DIR` at crate build time.
//!
//! Provides musl's `libc.a` and the crt start objects (`crt1.o`, `crti.o`,
//! `crtn.o`) that a fully static `-nostdlib` link needs for `_start`, and
//! copies them into Cargo's `OUT_DIR`, where `src/codegen/mod.rs` embeds
//! them with `include_bytes!`. That embedding is what lets a compiled
//! Cinnabar program link `-static -nostdlib` with no dependency on the
//! host's libc.
//!
//! When the `static-musl` feature is enabled on Linux, musl is provisioned
//! from upstream rather than taken from the host. The release tarball is
//! downloaded once into `target/musl/cache/`, its SHA-256 is checked against
//! the pin below, and the source is compiled with `clang` into
//! `target/musl/build/<version>/<arch>/install/`. Everything lives under the
//! cargo target directory, so `cargo clean` purges it and no binary artifact
//! is ever committed. Note: upstream reports CVE-2026-40200 affecting all
//! releases through 1.2.6 on 32-bit architectures; the arches used here are
//! 64-bit and are not affected.
//!
//! Discovery then runs in this order:
//!   1. the `MUSL_LIBC_A` environment variable (manual override),
//!   2. the self-provisioned cache above,
//!   3. standard host musl paths,
//!   4. the rustc sysroot.
//!
//! **Invariants:**
//! - No binary artifact ever lives in the source tree. Every archive is
//!   staged dynamically at build time from the order above, under the cargo
//!   target directory.
//! - The downloaded archive is pinned to an exact SHA-256 and verified
//!   before extraction; a mismatch aborts the build.
//! - Downloads and staging copies are atomic (temp file, then rename),
//!   because a partially written archive would fail at link time rather
//!   than here.
//! - This script generates no Rust code; it only stages files.
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const STAGE_NAMES: &[&str] = &["libc.a", "crt1.o", "crti.o", "crtn.o"];
const MUSL_VERSION: &str = "1.2.6";
const MUSL_ARCHIVE_URL: &str = "https://musl.libc.org/releases/musl-1.2.6.tar.gz";
const MUSL_SHA256: &str = "d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_STATIC_MUSL");
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_DIR");
    let target = env::var("TARGET")?;
    if env::var_os("CARGO_FEATURE_STATIC_MUSL").is_none() || !target.contains("linux") {
        println!("cargo:rerun-if-changed=build.rs");
        return Ok(());
    }
    let arch = musl_arch(&target)?;
    provision_musl(&arch)?;
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let lib_dir = find_musl_lib_dir(&arch)?;
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
                // Stage via a temp file + atomic rename so a partially
                // written archive is never observed at the final name.
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
//   2. the self-provisioned cache,
//   3. standard host musl paths,
//   4. the rustc sysroot.
fn find_musl_lib_dir(arch: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(path) = env::var("MUSL_LIBC_A") {
        let archive = PathBuf::from(&path);
        if archive.is_file()
            && let Some(dir) = archive.parent()
        {
            dirs.push(dir.to_path_buf());
        }
    }
    dirs.push(musl_install_lib(arch));
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
    Err(format!(
        "no musl libc.a found for architecture '{}': set MUSL_LIBC_A or install a rust musl target",
        arch
    )
    .into())
}

// Ensures a compiled musl exists in the self-provisioned cache, downloading
// and building it from source when the cache is empty. An explicit
// MUSL_LIBC_A override short-circuits this entirely: the developer is in
// manual control, so no network or compiler is touched.
fn provision_musl(arch: &str) -> Result<(), Box<dyn std::error::Error>> {
    if env::var_os("MUSL_LIBC_A").is_some() {
        return Ok(());
    }
    let install_lib = musl_install_lib(arch);
    if all_artifacts_present(&install_lib) {
        return Ok(());
    }
    let cache_dir = musl_root().join("cache");
    let archive = cache_dir.join(format!("musl-{}.tar.gz", MUSL_VERSION));
    ensure_archive(&cache_dir, &archive)?;
    let build_root = musl_root().join("build").join(MUSL_VERSION).join(arch);
    let src_dir = build_root.join("src");
    build_musl(arch, &archive, &src_dir, &build_root)?;
    if !all_artifacts_present(&install_lib) {
        return Err(format!(
            "musl build finished but the expected artifacts are missing from '{}'",
            install_lib.display()
        )
        .into());
    }
    Ok(())
}

// The directory the self-provisioned musl installs its libc.a and crt start
// objects into for one architecture.
fn musl_install_lib(arch: &str) -> PathBuf {
    musl_root()
        .join("build")
        .join(MUSL_VERSION)
        .join(arch)
        .join("install")
        .join("lib")
}

// `target/musl` under the cargo target directory (honoring CARGO_TARGET_DIR),
// so `cargo clean` purges every provisioned artifact.
fn musl_root() -> PathBuf {
    match env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir).join("musl"),
        None => PathBuf::from("target").join("musl"),
    }
}

// The architecture musl is being built for, read from the first component of
// the cargo target triple.
fn musl_arch(target: &str) -> Result<String, Box<dyn std::error::Error>> {
    match target.split('-').next() {
        Some(arch) => {
            if arch.is_empty() {
                Err(format!("cannot determine architecture from TARGET '{}'", target).into())
            } else {
                Ok(arch.to_string())
            }
        }
        None => Err(format!("cannot determine architecture from TARGET '{}'", target).into()),
    }
}

// Downloads the pinned release tarball (once), verifies its SHA-256, and
// leaves the verified archive at the cache path. A freshly downloaded archive
// is written to a temp file and renamed only after its checksum matches, so a
// corrupt download never becomes the cached archive.
fn ensure_archive(cache_dir: &Path, archive: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !archive.is_file() {
        fs::create_dir_all(cache_dir)?;
        let tmp = cache_dir.join(format!("musl-{}.tar.gz.tmp", MUSL_VERSION));
        download_archive(&tmp)?;
        let computed = sha256_of(&tmp)?;
        if computed != MUSL_SHA256 {
            remove_file_best_effort(&tmp);
            return Err(format!(
                "musl source archive checksum verification failed: expected {}, got {}",
                MUSL_SHA256, computed
            )
            .into());
        }
        fs::rename(&tmp, archive)?;
    }
    // The cached archive may predate this build; re-verify before use.
    let computed = sha256_of(archive)?;
    if computed != MUSL_SHA256 {
        remove_file_best_effort(archive);
        return Err(format!(
            "cached musl source archive checksum verification failed for '{}': expected {}, got {}",
            archive.display(),
            MUSL_SHA256,
            computed
        )
        .into());
    }
    Ok(())
}

// Fetches the release tarball to `dest`, trying curl first and falling back
// to wget. Each attempt propagates the tool's own failure; a download is
// never silently accepted as complete.
fn download_archive(dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let curl_result = fetch(
        dest,
        "curl",
        &["--fail", "--silent", "--show-error", "--location", "--output"],
    );
    if let Err(curl_err) = curl_result {
        let wget_result = fetch(dest, "wget", &["--quiet", "--output-document"]);
        if let Err(wget_err) = wget_result {
            return Err(format!(
                "cannot download '{}': curl failed ({}); wget failed ({})",
                MUSL_ARCHIVE_URL, curl_err, wget_err
            )
            .into());
        }
    }
    Ok(())
}

fn fetch(dest: &Path, tool: &str, base_args: &[&str]) -> Result<(), String> {
    let output = match Command::new(tool)
        .args(base_args)
        .arg(dest)
        .arg(MUSL_ARCHIVE_URL)
        .output()
    {
        Ok(output) => output,
        Err(err) => return Err(format!("{} is unavailable: {}", tool, err)),
    };
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} exited {}: {}",
            tool,
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

// The SHA-256 of a file on disk, as reported by `sha256sum`, as a lowercase
// hex string comparable to the pin.
fn sha256_of(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("sha256sum").arg(path).output()?;
    if !output.status.success() {
        return Err(format!(
            "sha256sum failed for '{}': {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    let stdout = String::from_utf8(output.stdout)?;
    match stdout.split_whitespace().next() {
        Some(hash) => Ok(hash.to_string()),
        None => Err(format!("sha256sum produced no output for '{}'", path.display()).into()),
    }
}

// Extracts the tarball into `src_dir`, stripping the leading
// `musl-<version>/` component so `configure` sits directly in `src_dir`.
fn extract_archive_to(archive: &Path, src_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(src_dir)?;
    let output = Command::new("tar")
        .args(["-xzf"])
        .arg(archive)
        .arg("-C")
        .arg(src_dir)
        .arg("--strip-components=1")
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "tar failed to extract '{}': {}",
            archive.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(())
}

// Configures, compiles, and installs musl into `build_root/install`. The
// configure is run with `clang` at `-O2` and `--disable-shared`, so only the
// static archive and crt start objects the staged link needs are produced.
fn build_musl(
    arch: &str,
    archive: &Path,
    src_dir: &Path,
    build_root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let install_dir = std::path::absolute(build_root.join("install"))?;
    fs::create_dir_all(build_root)?;
    extract_archive_to(archive, src_dir)?;
    let mut configure = Command::new("./configure");
    // The prefix must be absolute: `make install` runs with the source tree
    // as its cwd, and a relative prefix would be re-resolved against that
    // cwd, burying the install under `src/` instead of `build_root/install`.
    configure.arg(format!("--prefix={}", install_dir.display()));
    configure.arg("--disable-shared");
    if arch != std::env::consts::ARCH {
        configure.arg(format!("--target={}-linux-musl", arch));
    }
    configure
        .env("CC", "clang")
        .env("CFLAGS", "-O2")
        .current_dir(src_dir);
    run_build_tool(&mut configure, "musl configure")?;
    let mut make = Command::new("make");
    make.arg(format!("-j{}", parallel_jobs())).current_dir(src_dir);
    run_build_tool(&mut make, "musl make")?;
    let mut install = Command::new("make");
    install.arg("install").current_dir(src_dir);
    run_build_tool(&mut install, "musl make install")?;
    Ok(())
}

// Runs a build tool, turning a non-zero exit into an error that carries the
// tool's own output rather than a bare status code.
fn run_build_tool(command: &mut Command, what: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output = command.output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{} failed (exit {}): {} {}",
        what,
        output.status,
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    )
    .into())
}

// The number of parallel make jobs, read from the machine, falling back to a
// single job when the count cannot be detected.
fn parallel_jobs() -> usize {
    match std::thread::available_parallelism() {
        Ok(count) => count.get(),
        Err(err) => {
            eprintln!(
                "build.rs: cannot detect CPU count ({}); using a single job",
                err
            );
            1
        }
    }
}

// Whether every archive the staged link needs is already present in `lib_dir`.
fn all_artifacts_present(lib_dir: &Path) -> bool {
    let mut idx = 0usize;
    while idx < STAGE_NAMES.len() {
        match STAGE_NAMES.get(idx) {
            Some(name) => {
                if !lib_dir.join(name).is_file() {
                    return false;
                }
            }
            None => return false,
        }
        idx += 1;
    }
    true
}

// Removes a file, reporting (but not failing on) removal errors. Used to
// clean up a corrupt archive before aborting, where the checksum mismatch is
// the real error and a failed cleanup must not mask it.
fn remove_file_best_effort(path: &Path) {
    if let Err(err) = fs::remove_file(path) {
        eprintln!("build.rs: cannot remove '{}': {}", path.display(), err);
    }
}

// A directory counts as a musl lib dir when it holds libc.a either directly
// (musl's own layout, as installed by a host package or the self-provisioned
// build) or under the `self-contained` subdirectory that rustup uses for its
// musl targets. The staging loop resolves each archive against the same two
// locations.
fn holds_libc_a(dir: &Path) -> bool {
    dir.join("libc.a").is_file() || dir.join("self-contained").join("libc.a").is_file()
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
