use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Output};

pub const MANIFEST_FILE: &str = "build.cnb";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectManifest {
    pub root: PathBuf,
    pub entry: PathBuf,
    pub tests: PathBuf,
}

pub fn discover(start: &Path) -> Result<ProjectManifest, String> {
    let start_dir = if start.is_file() {
        match start.parent() {
            Some(parent) => parent.to_path_buf(),
            None => return Err(format!("cannot determine project directory for '{}'", start.display())),
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
    Err(format!("cannot find {} from '{}'", MANIFEST_FILE, start.display()))
}

pub fn entry_for_source(source: &Path) -> Option<PathBuf> {
    match discover(source) {
        Ok(manifest) => Some(manifest.entry),
        Err(message) => {
            if message.is_empty() {
                return None;
            }
            None
        }
    }
}

pub fn load_manifest(path: &Path) -> Result<ProjectManifest, String> {
    let source = fs::read_to_string(path)
        .map_err(|read_error| format!("cannot read project manifest '{}': {}", path.display(), read_error))?;
    let root = match path.parent() {
        Some(parent) => parent.to_path_buf(),
        None => return Err(format!("manifest '{}' has no project directory", path.display())),
    };
    let mut entry: Option<PathBuf> = None;
    let mut tests: Option<PathBuf> = None;
    for (line_index, raw_line) in source.lines().enumerate() {
        let content = match raw_line.split_once('#') {
            Some((before_comment, comment)) => {
                if comment.contains('\0') {
                    return Err(format!("{}:{}: comment contains a null byte", path.display(), line_index + 1));
                }
                before_comment.trim()
            }
            None => raw_line.trim(),
        };
        if content.is_empty() {
            continue;
        }
        let (key, value) = match content.split_once('=') {
            Some((raw_key, raw_value)) => (raw_key.trim(), raw_value.trim()),
            None => return Err(format!("{}:{}: expected 'key = relative/path'", path.display(), line_index + 1)),
        };
        let relative = validate_relative_path(value, path, line_index + 1)?;
        if key == "entry" {
            if entry.is_some() {
                return Err(format!("{}:{}: duplicate entry field", path.display(), line_index + 1));
            }
            entry = Some(relative);
        } else if key == "tests" {
            if tests.is_some() {
                return Err(format!("{}:{}: duplicate tests field", path.display(), line_index + 1));
            }
            tests = Some(relative);
        } else {
            return Err(format!("{}:{}: unknown manifest field '{}'", path.display(), line_index + 1, key));
        }
    }
    let entry_path = match entry {
        Some(relative) => root.join(relative),
        None => return Err(format!("{}: missing required entry field", path.display())),
    };
    let tests_path = match tests {
        Some(relative) => root.join(relative),
        None => root.join("tests"),
    };
    if !entry_path.is_file() {
        return Err(format!("project entry '{}' does not exist", entry_path.display()));
    }
    Ok(ProjectManifest { root, entry: entry_path, tests: tests_path })
}

fn validate_relative_path(value: &str, manifest: &Path, line: usize) -> Result<PathBuf, String> {
    if value.is_empty() {
        return Err(format!("{}:{}: path value cannot be empty", manifest.display(), line));
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Err(format!("{}:{}: project paths must be relative", manifest.display(), line));
    }
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                if name.is_empty() {
                    return Err(format!("{}:{}: empty path component", manifest.display(), line));
                }
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(format!("{}:{}: project paths cannot leave the project root", manifest.display(), line));
            }
            Component::RootDir => {
                return Err(format!("{}:{}: project paths must be relative", manifest.display(), line));
            }
            Component::Prefix(prefix) => {
                return Err(format!("{}:{}: path prefix '{:?}' is not allowed", manifest.display(), line, prefix));
            }
        }
    }
    Ok(path)
}

pub fn initialize(directory: &Path) -> Result<(), String> {
    let manifest = directory.join(MANIFEST_FILE);
    let main = directory.join("main.cnb");
    let tests_dir = directory.join("tests");
    let smoke = tests_dir.join("smoke.cnb");
    for target in [&manifest, &main, &smoke] {
        if target.exists() {
            return Err(format!("refusing to overwrite existing path '{}'", target.display()));
        }
    }
    fs::create_dir_all(&tests_dir)
        .map_err(|create_error| format!("cannot create project directory '{}': {}", tests_dir.display(), create_error))?;
    fs::write(&manifest, "entry = main.cnb\ntests = tests\n")
        .map_err(|write_error| format!("cannot write '{}': {}", manifest.display(), write_error))?;
    fs::write(&main, "pub fun main() I64\n  return 0\nend\n")
        .map_err(|write_error| format!("cannot write '{}': {}", main.display(), write_error))?;
    fs::write(&smoke, "pub fun main() I64\n  return 0\nend\n")
        .map_err(|write_error| format!("cannot write '{}': {}", smoke.display(), write_error))?;
    Ok(())
}

pub fn discover_tests(manifest: &ProjectManifest) -> Result<Vec<PathBuf>, String> {
    if !manifest.tests.is_dir() {
        return Err(format!("test directory '{}' does not exist", manifest.tests.display()));
    }
    let mut tests = Vec::new();
    collect_tests(&manifest.tests, &mut tests)?;
    tests.sort();
    Ok(tests)
}

fn collect_tests(directory: &Path, tests: &mut Vec<PathBuf>) -> Result<(), String> {
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

pub fn run_tests(executable: &Path, manifest: &ProjectManifest, update_snapshots: bool) -> Result<TestSummary, String> {
    let tests = discover_tests(manifest)?;
    let output_dir = manifest.root.join("target").join("cinnabar-tests");
    fs::create_dir_all(&output_dir)
        .map_err(|create_error| format!("cannot create test output directory '{}': {}", output_dir.display(), create_error))?;
    let mut passed = 0usize;
    let mut failed = Vec::new();
    for test in &tests {
        let project_relative = test.strip_prefix(&manifest.root)
            .map_err(|prefix_error| format!("cannot relativize test '{}': {}", test.display(), prefix_error))?;
        let test_relative = test.strip_prefix(&manifest.tests)
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
        let rejection = is_rejection_test(test) || snapshot.is_file();
        let result = if rejection {
            check_rejection(test, &snapshot, &compile, update_snapshots)
        } else {
            check_success(test, &binary, &compile)
        };
        match result {
            Ok(()) => passed += 1,
            Err(message) => failed.push(message),
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

fn check_rejection(test: &Path, snapshot: &Path, compile: &Output, update_snapshots: bool) -> Result<(), String> {
    if compile.status.success() {
        return Err(format!("{}: expected rejection but compilation succeeded", test.display()));
    }
    let actual = normalize_text(&String::from_utf8_lossy(&compile.stderr));
    if update_snapshots {
        fs::write(snapshot, &actual)
            .map_err(|write_error| format!("cannot update snapshot '{}': {}", snapshot.display(), write_error))?;
        return Ok(());
    }
    if snapshot.is_file() {
        let expected = fs::read_to_string(snapshot)
            .map_err(|read_error| format!("cannot read snapshot '{}': {}", snapshot.display(), read_error))?;
        if normalize_text(&expected) != actual {
            return Err(format!("{}: diagnostic snapshot differs from '{}'", test.display(), snapshot.display()));
        }
    }
    Ok(())
}

fn check_success(test: &Path, binary: &Path, compile: &Output) -> Result<(), String> {
    if !compile.status.success() {
        return Err(format!("{}: compilation failed\n{}", test.display(), String::from_utf8_lossy(&compile.stderr)));
    }
    let run_status = Command::new(binary)
        .current_dir(match test.parent() {
            Some(parent) => parent,
            None => test,
        })
        .status()
        .map_err(|spawn_error| format!("cannot run test '{}': {}", test.display(), spawn_error))?;
    let expected = expected_exit(test)?;
    status_matches(run_status, expected, test)
}

fn expected_exit(test: &Path) -> Result<i32, String> {
    let path = exit_path(test);
    if !path.is_file() {
        return Ok(0);
    }
    let text = fs::read_to_string(&path)
        .map_err(|read_error| format!("cannot read expected exit '{}': {}", path.display(), read_error))?;
    text.trim()
        .parse::<i32>()
        .map_err(|parse_error| format!("invalid exit status in '{}': {}", path.display(), parse_error))
}

fn status_matches(status: ExitStatus, expected: i32, test: &Path) -> Result<(), String> {
    match status.code() {
        Some(actual) => {
            if actual == expected {
                Ok(())
            } else {
                Err(format!("{}: expected exit {}, got {}", test.display(), expected, actual))
            }
        }
        None => Err(format!("{}: test terminated without an exit status", test.display())),
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

    #[test]
    fn manifest_paths_are_rooted_and_confined() {
        let root = test_directory("manifest");
        let create_result = fs::create_dir_all(&root);
        assert!(create_result.is_ok());
        let main_write = fs::write(root.join("main.cnb"), "pub fun main() I64\n  return 0\nend\n");
        assert!(main_write.is_ok());
        let manifest_write = fs::write(root.join(MANIFEST_FILE), "entry = main.cnb\ntests = tests\n");
        assert!(manifest_write.is_ok());
        let manifest = match load_manifest(&root.join(MANIFEST_FILE)) {
            Ok(value) => value,
            Err(message) => {
                assert!(message.is_empty(), "{}", message);
                return;
            }
        };
        assert_eq!(manifest.entry, root.join("main.cnb"));
        assert!(validate_relative_path("../escape", &root.join(MANIFEST_FILE), 1).is_err());
    }
}
