//! The editor extension's own suite, run as part of `cargo test`.
//!
//! The extension restates parts of the language that are not generated from
//! anything: the grammar's keyword lists, the comment tokens, the file
//! extension, the server binary name. Its suite compares those against the
//! compiler's own tables and drives a real LSP session against the built
//! server. Running under `cargo test` keeps it inside the gate's reach.
//!
//! **Invariants:**
//! - The suite runs through `node --test` with the TAP reporter so a pass
//!   count is assertable from stdout.

use std::path::{Path, PathBuf};
use std::process::Command;

fn extension_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("editors").join("vscode")
}

#[test]
fn vscode_extension_suite_passes() {
    let root = extension_root();
    if !root.join("package.json").is_file() {
        panic!("no extension manifest under {}", root.display());
    }

    // TAP's summary (`# pass <n>`) is stable enough to assert a count against.
    let output = match Command::new("node")
        .arg("--test")
        .arg("--test-reporter=tap")
        .current_dir(&root)
        .output()
    {
        Ok(value) => value,
        Err(err) => {
            panic!(
                "could not run 'node --test' in {}: {}.  Run the suite through \
                 'nix develop --command cargo test'.",
                root.display(),
                err
            );
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.status.success() {
        panic!(
            "the VS Code extension suite failed:\n{}\n{}",
            stdout,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // `node --test` exits 0 when it discovers nothing, so require a pass count.
    let passed = summary_count(&stdout, "# pass ");
    assert!(
        passed > 0,
        "the VS Code extension suite ran no tests; discovery is broken:\n{}",
        stdout
    );
}

// The trailing count on node's `# pass <n>` summary line.
fn summary_count(stdout: &str, prefix: &str) -> i64 {
    for line in stdout.lines() {
        if let Some(rest) = line.trim().strip_prefix(prefix) {
            return rest.trim().parse::<i64>().unwrap_or(0);
        }
    }
    0
}
