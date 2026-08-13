//! The `build.cnb` project manifest, and the project test runner.
//!
//! A manifest is not a configuration format. It is a Cinnabar program, read
//! through the compiler's own front end: `load_manifest` analyzes it,
//! refuses it if that analysis produced any diagnostic, and then reads its
//! three fields — `NAME`, `ENTRY`, `TESTS` — as `pub const` declarations of
//! type `&[U8]` whose values are the folded constants the typechecker
//! already computed. There is no second parser here, no second casing rule,
//! and no second notion of what a string literal is.
//!
//! Around that: `discover` walks upward from a path to the nearest
//! manifest, `initialize` writes a new project, and `discover_tests` and
//! `run_tests` compile and run each test file — comparing a rejection test
//! against its stored diagnostic snapshot and a success test against its
//! expected exit code.
//!
//! **Invariants:**
//! - Manifest paths stay inside the project root. A value that escapes it,
//!   or that names a reserved device, is a manifest error rather than a
//!   path the tooling goes on to act on.
//! - A manifest diagnostic carries the real span of the offending item,
//!   because `build.cnb` is a real source file with real spans.
//!   `ManifestError::source_less` exists for failures that genuinely have
//!   no source origin, and is not a shortcut for the ones that do.
//! - Whether a field exists is settled before whether its type is right.
//!   The other order tells the author of `pub const VERSION: I64` to change
//!   its type — asserting `VERSION` is a manifest field, which it is not,
//!   and sending them to fix the wrong thing first.

use crate::analysis;
use crate::ast::*;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Output};

pub const MANIFEST_FILE: &str = "build.cnb";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectManifest {
    pub name: String,
    pub root: PathBuf,
    pub entry: PathBuf,
    pub tests: PathBuf,
}

pub fn discover(start: &Path) -> Result<ProjectManifest, ManifestError> {
    let start_dir = if start.is_file() {
        match start.parent() {
            Some(parent) => parent.to_path_buf(),
            None => return Err(ManifestError::source_less(format!("cannot determine project directory for '{}'", start.display()))),
        }
    } else {
        start.to_path_buf()
    };
    let mut cursor = Some(start_dir.as_path());
    while let Some(directory) = cursor {
        let candidate = directory.join(MANIFEST_FILE);
        if candidate.is_file() {
            return load_manifest(&candidate);
        }
        cursor = directory.parent();
    }
    Err(ManifestError::source_less(format!("cannot find {} from '{}'", MANIFEST_FILE, start.display())))
}

/// The entry source of the project containing `source`.
///
/// The failure is handed back rather than swallowed. Whether "there is no
/// project here" is worth reporting is the caller's policy and not this
/// function's: the language server treats it as an ordinary answer for a file
/// that belongs to no project, while a caller that needs a project has the
/// diagnostic to render. Deciding here would mean either discarding a real
/// failure or printing one from inside a pipeline stage.
pub fn entry_for_source(source: &Path) -> Result<PathBuf, ManifestError> {
    discover(source).map(|manifest| manifest.entry)
}

pub fn load_manifest(path: &Path) -> Result<ProjectManifest, ManifestError> {
    let root_source = match path.parent() {
        Some(parent) => parent,
        None => return Err(ManifestError::source_less(format!("manifest '{}' has no project directory", path.display()))),
    };
    let root = fs::canonicalize(root_source)
        .map_err(|resolve_error| format!("cannot resolve project root '{}': {}", root_source.display(), resolve_error))?;
    let manifest_path = path.to_string_lossy().to_string();
    let analyzed = analysis::analyze(&manifest_path, &[]);
    if !analyzed.errors.is_empty() {
        return Err(ManifestError::from_front_end(&analyzed));
    }
    let manifest_file = analysis::file_id_of(&analyzed, &manifest_path);
    if manifest_file == NONE {
        return Err(ManifestError::source_less(format!("manifest '{}' was not present in its front-end analysis", path.display())));
    }
    let manifest_source = analysis::file_text_of(&analyzed, manifest_file);
    let manifest_span = (manifest_file, 0i64, manifest_source.len() as i64);
    let mut name: Option<String> = None;
    let mut entry: Option<PathBuf> = None;
    let mut tests: Option<PathBuf> = None;
    let item_count = list_len(&analyzed.lists, analyzed.root);
    let mut item_index = 0i64;
    while item_index < item_count {
        let item = list_get(&analyzed.lists, analyzed.root, item_index);
        if node_file(&analyzed.nodes, item) == NO_FILE {
            item_index += 1;
            continue;
        }
        if node_tag(&analyzed.nodes, item) != NODE_ITEM
            || node_a(&analyzed.nodes, item) != ITEM_CONST
            || item_is_pub(&analyzed.nodes, item) != 1
        {
            return Err(manifest_item_error(
                &analyzed,
                item,
                "manifest items must be pub const declarations",
            ));
        }
        // Whether the field exists is settled before whether its type is
        // right. The other order tells the author of `pub const VERSION: I64`
        // to change its type — asserting `VERSION` is a manifest field, which
        // it is not, and sending them to fix the wrong thing first.
        let field = name_text(&analyzed.names, node_d(&analyzed.nodes, item));
        if field != "NAME" && field != "ENTRY" && field != "TESTS" {
            return Err(manifest_item_error(
                &analyzed,
                item,
                &format!("unknown manifest field '{}'", field),
            ));
        }
        let declared_type = ty_key_of(&analyzed.nodes, node_e(&analyzed.nodes, item));
        if !is_manifest_string_type(&analyzed.nodes, declared_type) {
            return Err(manifest_item_error(
                &analyzed,
                item,
                "manifest fields must have declared type '&[U8]'",
            ));
        }
        let symbol = item_sym_of(&analyzed.nodes, item);
        if !has_const_value(&analyzed.nodes, symbol) {
            return Err(manifest_item_error(&analyzed, item, "manifest field has no folded constant value"));
        }
        let folded = find_const_value(&analyzed.nodes, symbol);
        let value = match analyzed.names.get(folded as usize) {
            Some(text) => text.clone(),
            None => return Err(manifest_item_error(&analyzed, item, "manifest field has an invalid folded value")),
        };
        // Three fields, and the check above has already rejected anything
        // else, so the final arm is `TESTS` rather than a fourth case that
        // cannot happen.
        if field == "NAME" {
            name = Some(validate_project_name(&value, &analyzed, item)?);
        } else if field == "ENTRY" {
            entry = Some(validate_relative_path(&value, &analyzed, item)?);
        } else {
            tests = Some(validate_relative_path(&value, &analyzed, item)?);
        }
        item_index += 1;
    }
    let project_name = match name {
        Some(value) => value,
        None => return Err(manifest_span_error(&analyzed, manifest_span, "missing required NAME field")),
    };
    let entry_source = match entry {
        Some(relative) => root.join(relative),
        None => return Err(manifest_span_error(&analyzed, manifest_span, "missing required ENTRY field")),
    };
    let tests_path = match tests {
        Some(relative) => root.join(relative),
        None => root.join("tests"),
    };
    let entry_path = canonicalize_confined_existing(&root, &entry_source, "project entry")?;
    if !entry_path.is_file() {
        return Err(ManifestError::source_less(format!("project entry '{}' is not a file", entry_path.display())));
    }
    Ok(ProjectManifest { name: project_name, root, entry: entry_path, tests: tests_path })
}

/// The project name names the built artifact, so it has to be one path
/// component and nothing else.
///
/// A name carrying a separator, a parent-directory step, or a drive prefix
/// would let a manifest choose where the build writes rather than merely
/// what it is called. `ENTRY` and `TESTS` are confined to the project root
/// because a path is obviously a path; `NAME` reaches the same filesystem
/// through a field that does not look like one, which is exactly why it is
/// checked here rather than trusted.
fn validate_project_name(value: &str, analyzed: &analysis::Analysis, item: i64) -> Result<String, ManifestError> {
    // What the first component *is* decides the message. Counting components
    // first would answer "../outside" with a complaint about its length,
    // which is true and useless: the problem is the step out of the root, and
    // that is what the manifest author has to be told.
    let mut components = Path::new(value).components();
    let name = match components.next() {
        Some(Component::Normal(text)) => match text.to_str() {
            Some(text) => text.to_string(),
            None => return Err(manifest_item_error(analyzed, item, "project name is not valid text")),
        },
        Some(Component::CurDir) => {
            return Err(manifest_item_error(analyzed, item, "project name cannot be a directory reference"));
        }
        Some(Component::ParentDir) => {
            return Err(manifest_item_error(analyzed, item, "project name cannot leave the project root"));
        }
        Some(Component::RootDir) => {
            return Err(manifest_item_error(analyzed, item, "project name must be relative"));
        }
        Some(Component::Prefix(prefix)) => {
            return Err(manifest_item_error(
                analyzed,
                item,
                &format!("project name cannot carry the path prefix '{:?}'", prefix),
            ));
        }
        None => return Err(manifest_item_error(analyzed, item, "project name cannot be empty")),
    };
    if let Some(trailing) = components.next() {
        return Err(manifest_item_error(
            analyzed,
            item,
            &format!("project name must be a single path component, but continues with '{:?}'", trailing),
        ));
    }
    // A single component is still not enough on Windows. `NUL`, `CON`, the
    // `COM`/`LPT` series, and any name with a trailing dot or space name
    // devices rather than files, in *every* directory — so `NAME = "NUL"`
    // would let a build report success while its artifact went nowhere. The
    // check is unconditional rather than `cfg(windows)`: a manifest is
    // portable, and a project that builds on Linux and vanishes on Windows
    // is worse than one rejected in both places.
    if is_reserved_device_name(&name) {
        return Err(manifest_item_error(
            analyzed,
            item,
            &format!("project name '{}' names a reserved device on Windows rather than a file", name),
        ));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(manifest_item_error(
            analyzed,
            item,
            "project name cannot end with a dot or a space; Windows strips both, so the name on disk would differ from the one declared",
        ));
    }
    Ok(name)
}

/// Whether `name` is a Windows reserved device name.
///
/// Matched on the stem before the first dot and without regard to case,
/// because `NUL`, `nul`, and `nul.txt` all name the same device.
fn is_reserved_device_name(name: &str) -> bool {
    let stem = match name.split('.').next() {
        Some(text) => text,
        None => name,
    };
    let upper = stem.to_ascii_uppercase();
    if upper == "CON" || upper == "PRN" || upper == "AUX" || upper == "NUL" {
        return true;
    }
    let numbered = |prefix: &str| -> bool {
        match upper.strip_prefix(prefix) {
            Some(digit) => digit.len() == 1 && matches!(digit.as_bytes().first(), Some(b'1'..=b'9')),
            None => false,
        }
    };
    numbered("COM") || numbered("LPT")
}

fn is_manifest_string_type(nodes: &[i64], key: i64) -> bool {
    let reference = find_tyinfo(nodes, key);
    if reference == NONE || node_b(nodes, reference) != TYD_REF {
        return false;
    }
    let slice = find_tyinfo(nodes, node_e(nodes, reference));
    if slice == NONE || node_b(nodes, slice) != TYD_SLICE {
        return false;
    }
    let byte = find_tyinfo(nodes, node_e(nodes, slice));
    byte != NONE && node_b(nodes, byte) == TYD_BUILTIN && node_f(nodes, byte) == BUILTIN_U8
}

/// A manifest failure, carried as the compiler's own diagnostics.
///
/// `build.cnb` is Cinnabar source, so its errors have real spans in a real
/// file and belong in the same ariadne report as every other diagnostic.
/// Flattening them to a string here did two forbidden things at once: it
/// stringified an error before the final diagnostic, and it put a second
/// byte-offset-to-line-and-column implementation beside the one the compiler
/// already has.
///
/// A failure with no Cinnabar origin — a directory that cannot be read, a
/// manifest that is not there — carries `NO_FILE`, which the renderer already
/// understands. That is a source-less origin represented explicitly rather
/// than faked with a span into a file it did not come from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestError {
    pub diagnostics: Vec<Diag>,
    pub files: Vec<(String, String)>,
}

impl ManifestError {
    /// A failure with no Cinnabar source behind it.
    pub fn source_less(message: String) -> ManifestError {
        ManifestError { diagnostics: vec![(message, NO_FILE, 0, 0)], files: Vec::new() }
    }

    /// Everything the front end reported about the manifest, unchanged.
    fn from_front_end(analyzed: &analysis::Analysis) -> ManifestError {
        ManifestError { diagnostics: analyzed.errors.clone(), files: analyzed.files.clone() }
    }

    /// The first message, for callers that want text rather than a report.
    pub fn message(&self) -> String {
        match self.diagnostics.first() {
            Some(diagnostic) => diagnostic.0.clone(),
            None => "manifest failed without a diagnostic".to_string(),
        }
    }
}

// `?` converts a source-less failure automatically, which is what the
// filesystem errors throughout this module are.
impl From<String> for ManifestError {
    fn from(message: String) -> ManifestError {
        ManifestError::source_less(message)
    }
}

fn manifest_item_error(analyzed: &analysis::Analysis, item: i64, message: &str) -> ManifestError {
    manifest_span_error(
        analyzed,
        (
            node_file(&analyzed.nodes, item),
            node_start(&analyzed.nodes, item),
            node_end(&analyzed.nodes, item),
        ),
        message,
    )
}

fn manifest_span_error(analyzed: &analysis::Analysis, span: (i64, i64, i64), message: &str) -> ManifestError {
    ManifestError {
        diagnostics: vec![(message.to_string(), span.0, span.1, span.2)],
        files: analyzed.files.clone(),
    }
}

fn canonicalize_confined_existing(root: &Path, path: &Path, role: &str) -> Result<PathBuf, ManifestError> {
    let resolved = fs::canonicalize(path)
        .map_err(|resolve_error| format!("cannot resolve {} '{}': {}", role, path.display(), resolve_error))?;
    if !resolved.starts_with(root) {
        return Err(ManifestError::source_less(format!("{} '{}' resolves outside project root '{}'", role, path.display(), root.display())));
    }
    Ok(resolved)
}

fn validate_confined_sidecar(root: &Path, path: &Path, role: &str) -> Result<(), ManifestError> {
    if sidecar_present(path, role)? {
        let resolved = canonicalize_confined_existing(root, path, role)?;
        if !resolved.is_file() {
            return Err(ManifestError::source_less(format!("{} '{}' is not a file", role, path.display())));
        }
        return Ok(());
    }
    let parent = match path.parent() {
        Some(value) => value,
        None => return Err(ManifestError::source_less(format!("{} '{}' has no parent directory", role, path.display()))),
    };
    let resolved_parent = canonicalize_confined_existing(root, parent, role)?;
    if !resolved_parent.is_dir() {
        return Err(ManifestError::source_less(format!("{} parent '{}' is not a directory", role, parent.display())));
    }
    Ok(())
}

fn sidecar_present(path: &Path, role: &str) -> Result<bool, ManifestError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            let recognized_path = metadata.is_file() || metadata.is_symlink() || metadata.is_dir();
            if !recognized_path {
                return Err(ManifestError::source_less(format!("cannot classify {} '{}'", role, path.display())));
            }
            Ok(true)
        }
        Err(inspect_error) => {
            if inspect_error.kind() == std::io::ErrorKind::NotFound {
                Ok(false)
            } else {
                Err(ManifestError::source_less(format!("cannot inspect {} '{}': {}", role, path.display(), inspect_error)))
            }
        }
    }
}

fn validate_relative_path(value: &str, analyzed: &analysis::Analysis, item: i64) -> Result<PathBuf, ManifestError> {
    if value.is_empty() {
        return Err(manifest_item_error(analyzed, item, "path value cannot be empty"));
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Err(manifest_item_error(analyzed, item, "project paths must be relative"));
    }
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                if name.is_empty() {
                    return Err(manifest_item_error(analyzed, item, "empty path component"));
                }
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(manifest_item_error(analyzed, item, "project paths cannot leave the project root"));
            }
            Component::RootDir => {
                return Err(manifest_item_error(analyzed, item, "project paths must be relative"));
            }
            Component::Prefix(prefix) => {
                return Err(manifest_item_error(
                    analyzed,
                    item,
                    &format!("path prefix '{:?}' is not allowed", prefix),
                ));
            }
        }
    }
    Ok(path)
}

pub fn initialize(directory: &Path) -> Result<(), ManifestError> {
    let manifest = directory.join(MANIFEST_FILE);
    let main = directory.join("main.cnb");
    let tests_dir = directory.join("tests");
    let smoke = tests_dir.join("smoke.cnb");
    for target in [&manifest, &main, &smoke] {
        if target.exists() {
            return Err(ManifestError::source_less(format!("refusing to overwrite existing path '{}'", target.display())));
        }
    }
    fs::create_dir_all(&tests_dir)
        .map_err(|create_error| format!("cannot create project directory '{}': {}", tests_dir.display(), create_error))?;
    let project_name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("cinnabar");
    let manifest_source = format!(
        "pub const NAME: &[U8] = \"{}\"\npub const ENTRY: &[U8] = \"main.cnb\"\npub const TESTS: &[U8] = \"tests\"\n",
        escaped_literal_text(project_name),
    );
    fs::write(&manifest, manifest_source)
        .map_err(|write_error| format!("cannot write '{}': {}", manifest.display(), write_error))?;
    fs::write(&main, "pub fun main() I64\n  return 0\nend\n")
        .map_err(|write_error| format!("cannot write '{}': {}", main.display(), write_error))?;
    fs::write(&smoke, "pub fun main() I64\n  return 0\nend\n")
        .map_err(|write_error| format!("cannot write '{}': {}", smoke.display(), write_error))?;
    Ok(())
}

pub fn discover_tests(manifest: &ProjectManifest) -> Result<Vec<PathBuf>, ManifestError> {
    let tests_root = canonicalize_confined_existing(&manifest.root, &manifest.tests, "test directory")?;
    if !tests_root.is_dir() {
        return Err(ManifestError::source_less(format!("test directory '{}' is not a directory", tests_root.display())));
    }
    let mut tests = Vec::new();
    collect_tests(&tests_root, &mut tests)?;
    tests.sort();
    Ok(tests)
}

fn collect_tests(directory: &Path, tests: &mut Vec<PathBuf>) -> Result<(), ManifestError> {
    let entries = fs::read_dir(directory)
        .map_err(|read_error| format!("cannot read test directory '{}': {}", directory.display(), read_error))?;
    for entry_result in entries {
        let entry = entry_result
            .map_err(|entry_error| format!("cannot read entry in '{}': {}", directory.display(), entry_error))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|type_error| format!("cannot inspect '{}': {}", path.display(), type_error))?;
        if file_type.is_dir() {
            collect_tests(&path, tests)?;
        } else if file_type.is_file() && path.extension().and_then(|extension| extension.to_str()) == Some("cnb") {
            tests.push(path);
        } else {
            let recognized_non_file = file_type.is_symlink();
            if recognized_non_file {
                continue;
            }
        }
    }
    Ok(())
}

pub fn run_tests(executable: &Path, manifest: &ProjectManifest, update_snapshots: bool) -> Result<TestSummary, ManifestError> {
    let tests_root = canonicalize_confined_existing(&manifest.root, &manifest.tests, "test directory")?;
    let tests = discover_tests(manifest)?;
    let output_dir = manifest.root.join("target").join("cinnabar-tests");
    fs::create_dir_all(&output_dir)
        .map_err(|create_error| format!("cannot create test output directory '{}': {}", output_dir.display(), create_error))?;
    let mut passed = 0usize;
    let mut failed = Vec::new();
    for test in &tests {
        let project_relative = test.strip_prefix(&manifest.root)
            .map_err(|prefix_error| format!("cannot relativize test '{}': {}", test.display(), prefix_error))?;
        let test_relative = test.strip_prefix(&tests_root)
            .map_err(|prefix_error| format!("cannot relativize test '{}': {}", test.display(), prefix_error))?;
        let binary_name = test_relative.to_string_lossy().replace(['/', '\\'], "__");
        let binary = output_dir.join(binary_name).with_extension("bin");
        let compile = Command::new(executable)
            .current_dir(&manifest.root)
            .arg(project_relative)
            .arg("-o")
            .arg(&binary)
            .output()
            .map_err(|spawn_error| format!("cannot run compiler for '{}': {}", test.display(), spawn_error))?;
        let snapshot = snapshot_path(test);
        let rejection = is_rejection_test(test) || sidecar_present(&snapshot, "diagnostic snapshot")?;
        let result = if rejection {
            check_rejection(&manifest.root, test, &snapshot, &compile, update_snapshots)
        } else {
            check_success(&manifest.root, test, &binary, &compile)
        };
        match result {
            Ok(()) => passed += 1,
            Err(failure) => failed.push(failure.message()),
        }
    }
    Ok(TestSummary { discovered: tests.len(), passed, failed })
}

#[derive(Debug, Eq, PartialEq)]
pub struct TestSummary {
    pub discovered: usize,
    pub passed: usize,
    pub failed: Vec<String>,
}

fn is_rejection_test(path: &Path) -> bool {
    match path.file_name().and_then(|name| name.to_str()) {
        Some(name) => name.ends_with(".reject.cnb"),
        None => false,
    }
}

fn snapshot_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.stderr", path.display()))
}

fn exit_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.exit", path.display()))
}

fn check_rejection(root: &Path, test: &Path, snapshot: &Path, compile: &Output, update_snapshots: bool) -> Result<(), ManifestError> {
    if compile.status.success() {
        return Err(ManifestError::source_less(format!("{}: expected rejection but compilation succeeded", test.display())));
    }
    let actual = normalize_text(&String::from_utf8_lossy(&compile.stderr));
    if update_snapshots {
        validate_confined_sidecar(root, snapshot, "diagnostic snapshot")?;
        fs::write(snapshot, &actual)
            .map_err(|write_error| format!("cannot update snapshot '{}': {}", snapshot.display(), write_error))?;
        return Ok(());
    }
    if sidecar_present(snapshot, "diagnostic snapshot")? {
        validate_confined_sidecar(root, snapshot, "diagnostic snapshot")?;
        let expected = fs::read_to_string(snapshot)
            .map_err(|read_error| format!("cannot read snapshot '{}': {}", snapshot.display(), read_error))?;
        if normalize_text(&expected) != actual {
            return Err(ManifestError::source_less(format!("{}: diagnostic snapshot differs from '{}'", test.display(), snapshot.display())));
        }
    }
    Ok(())
}

fn check_success(root: &Path, test: &Path, binary: &Path, compile: &Output) -> Result<(), ManifestError> {
    if !compile.status.success() {
        return Err(ManifestError::source_less(format!("{}: compilation failed\n{}", test.display(), String::from_utf8_lossy(&compile.stderr))));
    }
    let run_status = Command::new(binary)
        .current_dir(match test.parent() {
            Some(parent) => parent,
            None => test,
        })
        .status()
        .map_err(|spawn_error| format!("cannot run test '{}': {}", test.display(), spawn_error))?;
    let expected = expected_exit(root, test)?;
    status_matches(run_status, expected, test)
}

fn expected_exit(root: &Path, test: &Path) -> Result<i32, ManifestError> {
    let path = exit_path(test);
    if !sidecar_present(&path, "expected exit sidecar")? {
        return Ok(0);
    }
    validate_confined_sidecar(root, &path, "expected exit sidecar")?;
    let text = fs::read_to_string(&path)
        .map_err(|read_error| format!("cannot read expected exit '{}': {}", path.display(), read_error))?;
    text.trim()
        .parse::<i32>()
        .map_err(|parse_error| ManifestError::source_less(format!("invalid exit status in '{}': {}", path.display(), parse_error)))
}

fn status_matches(status: ExitStatus, expected: i32, test: &Path) -> Result<(), ManifestError> {
    match status.code() {
        Some(actual) => {
            if actual == expected {
                Ok(())
            } else {
                Err(ManifestError::source_less(format!("{}: expected exit {}, got {}", test.display(), expected, actual)))
            }
        }
        None => Err(ManifestError::source_less(format!("{}: test terminated without an exit status", test.display()))),
    }
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_directory(label: &str) -> PathBuf {
        let timestamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_nanos(),
            Err(time_error) => time_error.duration().as_nanos(),
        };
        std::env::temp_dir().join(format!("cinnabar_project_{}_{}_{}", label, std::process::id(), timestamp))
    }

    fn write_project(root: &Path, manifest_source: &str) {
        assert!(fs::create_dir_all(root).is_ok());
        assert!(fs::write(root.join("main.cnb"), "pub fun main() I64\n  return 0\nend\n").is_ok());
        assert!(fs::write(root.join(MANIFEST_FILE), manifest_source).is_ok());
    }

    fn rejected_manifest(label: &str, manifest_source: &str) -> ManifestError {
        let root = test_directory(label);
        write_project(&root, manifest_source);
        match load_manifest(&root.join(MANIFEST_FILE)) {
            Ok(manifest) => {
                assert!(false, "manifest was accepted, name '{}'", manifest.name);
                ManifestError::source_less(String::new())
            }
            Err(failure) => failure,
        }
    }

    /// Asserts the rejection says `message`, and says it *somewhere real*.
    ///
    /// The span is checked rather than a rendered "file:line:col" prefix.
    /// That prefix used to come from a renderer inside this module, and
    /// removing it is the point: the diagnostic now carries its own span to
    /// the compiler's reporter, so the span is what there is to assert.
    fn assert_rejected_at_source(failure: &ManifestError, message: &str) {
        let diagnostic = match failure.diagnostics.first() {
            Some(value) => value,
            None => {
                assert!(false, "rejected with no diagnostic at all");
                return;
            }
        };
        assert!(
            diagnostic.0.contains(message),
            "rejected with '{}', want something containing '{}'",
            diagnostic.0,
            message
        );
        assert!(
            diagnostic.1 != NO_FILE,
            "'{}' carries no source file, but it is about the manifest's own text",
            diagnostic.0
        );
        let source = match failure.files.get(diagnostic.1 as usize) {
            Some(value) => value,
            None => {
                assert!(false, "'{}' names file {} which is not in the file table", diagnostic.0, diagnostic.1);
                return;
            }
        };
        assert!(
            diagnostic.2 >= 0 && diagnostic.3 >= diagnostic.2 && diagnostic.3 as usize <= source.1.len(),
            "'{}' spans {}..{} of a {}-byte manifest",
            diagnostic.0,
            diagnostic.2,
            diagnostic.3,
            source.1.len()
        );
    }

    #[test]
    fn cinnabar_manifest_parses_folded_string_fields() {
        let root = test_directory("manifest");
        write_project(
            &root,
            "pub const NAME: &[U8] = \"cinnabar\"\npub const ENTRY: &[U8] = \"main.cnb\"\npub const TESTS: &[U8] = \"tests\"\n",
        );
        let manifest = match load_manifest(&root.join(MANIFEST_FILE)) {
            Ok(value) => value,
            Err(failure) => {
                assert!(false, "{}", failure.message());
                return;
            }
        };
        assert_eq!(manifest.name, "cinnabar");
        assert_eq!(manifest.entry, root.join("main.cnb"));
        assert_eq!(manifest.tests, root.join("tests"));
    }

    #[test]
    fn missing_required_manifest_field_has_source_location() {
        let failure = rejected_manifest(
            "missing_entry",
            "pub const NAME: &[U8] = \"cinnabar\"\n",
        );
        assert_rejected_at_source(&failure, "missing required ENTRY field");
    }

    #[test]
    fn omitted_tests_field_uses_tests_directory() {
        let root = test_directory("default_tests");
        write_project(
            &root,
            "pub const NAME: &[U8] = \"cinnabar\"\npub const ENTRY: &[U8] = \"main.cnb\"\n",
        );
        let manifest = match load_manifest(&root.join(MANIFEST_FILE)) {
            Ok(value) => value,
            Err(failure) => {
                assert!(false, "{}", failure.message());
                return;
            }
        };
        assert_eq!(manifest.tests, root.join("tests"));
    }

    #[test]
    fn duplicate_manifest_field_has_source_location() {
        let failure = rejected_manifest(
            "duplicate_entry",
            "pub const NAME: &[U8] = \"cinnabar\"\npub const ENTRY: &[U8] = \"main.cnb\"\npub const ENTRY: &[U8] = \"other.cnb\"\n",
        );
        assert_rejected_at_source(&failure, "duplicate symbol 'ENTRY'");
    }

    #[test]
    fn wrong_manifest_field_type_has_declaration_location() {
        let failure = rejected_manifest(
            "wrong_type",
            "pub const NAME: &[U8] = \"cinnabar\"\npub const ENTRY: I64 = 1\n",
        );
        assert_rejected_at_source(&failure, "manifest fields must have declared type '&[U8]'");
    }

    #[test]
    fn manifest_path_cannot_escape_project_root() {
        let failure = rejected_manifest(
            "escape",
            "pub const NAME: &[U8] = \"cinnabar\"\npub const ENTRY: &[U8] = \"../outside.cnb\"\n",
        );
        assert_rejected_at_source(&failure, "project paths cannot leave the project root");
    }

    // NAME names the built artifact, so it reaches the filesystem through a
    // field that does not look like a path. A name that escapes its single
    // component would choose where the build writes.
    #[test]
    fn project_name_cannot_reach_outside_its_own_component() {
        let escaping = rejected_manifest(
            "name_escape",
            "pub const NAME: &[U8] = \"../outside\"\npub const ENTRY: &[U8] = \"main.cnb\"\n",
        );
        assert_rejected_at_source(&escaping, "project name cannot leave the project root");

        let nested = rejected_manifest(
            "name_nested",
            "pub const NAME: &[U8] = \"nested/name\"\npub const ENTRY: &[U8] = \"main.cnb\"\n",
        );
        assert_rejected_at_source(&nested, "project name must be a single path component");

        let empty = rejected_manifest(
            "name_empty",
            "pub const NAME: &[U8] = \"\"\npub const ENTRY: &[U8] = \"main.cnb\"\n",
        );
        assert_rejected_at_source(&empty, "project name cannot be empty");
    }

    #[test]
    fn invalid_cinnabar_manifest_reports_frontend_diagnostic() {
        let failure = rejected_manifest("invalid_source", "entry = main.cnb\n");
        assert!(failure.diagnostics.first().is_some(), "no diagnostic reported");
        assert!(!failure.message().contains("expected 'key = relative/path'"), "{}", failure.message());
    }

    #[test]
    fn manifest_rejects_items_that_are_not_public_constants() {
        let failure = rejected_manifest(
            "private_const",
            "const NAME: &[U8] = \"cinnabar\"\npub const ENTRY: &[U8] = \"main.cnb\"\n",
        );
        assert_rejected_at_source(&failure, "manifest items must be pub const declarations");
    }

    #[test]
    fn initialize_writes_loadable_cinnabar_manifest() {
        let root = test_directory("initialize");
        assert!(initialize(&root).is_ok());
        let source = match fs::read_to_string(root.join(MANIFEST_FILE)) {
            Ok(value) => value,
            Err(read_error) => {
                assert!(read_error.kind() == std::io::ErrorKind::NotFound, "{}", read_error);
                return;
            }
        };
        assert!(source.contains("pub const NAME: &[U8]"));
        assert!(source.contains("pub const ENTRY: &[U8] = \"main.cnb\""));
        assert!(source.contains("pub const TESTS: &[U8] = \"tests\""));
        let manifest = load_manifest(&root.join(MANIFEST_FILE));
        assert!(manifest.is_ok(), "{:?}", manifest);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_test_directory_symlink_outside_project() {
        use std::os::unix::fs::symlink;

        let root = test_directory("test_symlink_project");
        let outside = test_directory("test_symlink_outside");
        assert!(fs::create_dir_all(&root).is_ok());
        assert!(fs::create_dir_all(&outside).is_ok());
        assert!(fs::write(root.join("main.cnb"), "pub fun main() I64\n  return 0\nend\n").is_ok());
        assert!(fs::write(outside.join("escape.cnb"), "pub fun main() I64\n  return 0\nend\n").is_ok());
        assert!(symlink(&outside, root.join("tests")).is_ok());
        assert!(fs::write(
            root.join(MANIFEST_FILE),
            "pub const NAME: &[U8] = \"cinnabar\"\npub const ENTRY: &[U8] = \"main.cnb\"\npub const TESTS: &[U8] = \"tests\"\n",
        )
        .is_ok());

        let manifest = match load_manifest(&root.join(MANIFEST_FILE)) {
            Ok(value) => value,
            Err(failure) => {
                assert!(false, "{}", failure.message());
                return;
            }
        };
        let result = discover_tests(&manifest);
        assert!(result.is_err());
        let message = match result {
            Ok(paths) => {
                assert!(paths.is_empty());
                return;
            }
            Err(value) => value,
        };
        assert!(message.message().contains("resolves outside project root"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_snapshot_symlink_outside_project() {
        use std::os::unix::fs::symlink;

        let root = test_directory("snapshot_symlink_project");
        let outside = test_directory("snapshot_symlink_outside");
        assert!(fs::create_dir_all(root.join("tests")).is_ok());
        assert!(fs::create_dir_all(&outside).is_ok());
        let external_snapshot = outside.join("captured.stderr");
        assert!(fs::write(&external_snapshot, "outside\n").is_ok());
        let snapshot = root.join("tests").join("case.reject.cnb.stderr");
        assert!(symlink(&external_snapshot, &snapshot).is_ok());

        let result = validate_confined_sidecar(&root, &snapshot, "diagnostic snapshot");
        assert!(result.is_err());

        let dangling_snapshot = root.join("tests").join("dangling.reject.cnb.stderr");
        let absent_external = outside.join("absent.stderr");
        assert!(symlink(&absent_external, &dangling_snapshot).is_ok());
        let dangling_result = validate_confined_sidecar(&root, &dangling_snapshot, "diagnostic snapshot");
        assert!(dangling_result.is_err());
    }
}
