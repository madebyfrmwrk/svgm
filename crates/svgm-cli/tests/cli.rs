use std::fs;
use std::process::Command;

use tempfile::TempDir;

fn svgm() -> Command {
    Command::new(env!("CARGO_BIN_EXE_svgm"))
}

/// An SVG with enough redundancy that the optimizer will actually change it.
const TEST_SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100.000 100.000\">\n  \
    <!-- a comment -->\n  \
    <metadata>some metadata</metadata>\n  \
    <g>\n    \
        <rect x=\"10\" y=\"10\" width=\"80.000\" height=\"80.000\" fill=\"#ff0000\" opacity=\"1\"/>\n  \
    </g>\n\
</svg>";

fn write_svg(dir: &TempDir, name: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, TEST_SVG).unwrap();
    path
}

// ── Basic file operations ──────────────────────────────────────────────

#[test]
fn optimize_in_place() {
    let dir = TempDir::new().unwrap();
    let file = write_svg(&dir, "a.svg");

    // Use -o to force file output (in test context, stdout is not a terminal,
    // so single-file mode without -o would write to stdout via pipe detection).
    let output = svgm()
        .arg(&file)
        .args(["-o", file.to_str().unwrap()])
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", lossy(&output.stderr));

    let result = fs::read_to_string(&file).unwrap();
    assert_ne!(result, TEST_SVG, "file should have been modified");
    assert!(result.starts_with("<svg"), "output should be valid SVG");
}

#[test]
fn optimize_to_output_path() {
    let dir = TempDir::new().unwrap();
    let file = write_svg(&dir, "a.svg");
    let out = dir.path().join("out.svg");

    let output = svgm()
        .arg(&file)
        .args(["-o", out.to_str().unwrap()])
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", lossy(&output.stderr));

    // Original unchanged
    assert_eq!(fs::read_to_string(&file).unwrap(), TEST_SVG);
    // Output exists and is valid SVG
    let result = fs::read_to_string(&out).unwrap();
    assert!(result.starts_with("<svg"));
}

#[test]
fn dry_run_does_not_modify() {
    let dir = TempDir::new().unwrap();
    let file = write_svg(&dir, "a.svg");

    let output = svgm()
        .arg(&file)
        .arg("--dry-run")
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(output.status.success());

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        TEST_SVG,
        "file should not be modified in dry-run mode"
    );
}

// ── Stdout / pipe ──────────────────────────────────────────────────────

#[test]
fn stdout_flag_single_file() {
    let dir = TempDir::new().unwrap();
    let file = write_svg(&dir, "a.svg");

    let output = svgm()
        .arg(&file)
        .arg("--stdout")
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = lossy(&output.stdout);
    assert!(stdout.starts_with("<svg"), "stdout should contain SVG");
}

#[test]
fn stdout_with_multiple_files_errors() {
    let dir = TempDir::new().unwrap();
    let a = write_svg(&dir, "a.svg");
    let b = write_svg(&dir, "b.svg");

    let output = svgm()
        .arg(&a)
        .arg(&b)
        .arg("--stdout")
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        lossy(&output.stderr).contains("cannot use --stdout with multiple"),
        "stderr: {}",
        lossy(&output.stderr)
    );
}

#[test]
fn multi_file_optimizes_in_place() {
    let dir = TempDir::new().unwrap();
    let a = write_svg(&dir, "a.svg");
    let b = write_svg(&dir, "b.svg");

    let output = svgm().arg(&a).arg(&b).arg("--no-config").output().unwrap();
    assert!(output.status.success(), "stderr: {}", lossy(&output.stderr));

    // Both files should have been modified in place
    assert_ne!(fs::read_to_string(&a).unwrap(), TEST_SVG);
    assert_ne!(fs::read_to_string(&b).unwrap(), TEST_SVG);
}

// ── Recursive directory mode ───────────────────────────────────────────

#[test]
fn recursive_in_place() {
    let dir = TempDir::new().unwrap();
    write_svg(&dir, "sub/a.svg");
    write_svg(&dir, "b.svg");

    let output = svgm()
        .args(["-r", dir.path().to_str().unwrap()])
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", lossy(&output.stderr));

    let a = fs::read_to_string(dir.path().join("sub/a.svg")).unwrap();
    let b = fs::read_to_string(dir.path().join("b.svg")).unwrap();
    assert_ne!(a, TEST_SVG, "sub/a.svg should be optimized");
    assert_ne!(b, TEST_SVG, "b.svg should be optimized");
}

#[test]
fn recursive_with_output_dir() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(src.join("sub")).unwrap();
    fs::write(src.join("sub/a.svg"), TEST_SVG).unwrap();
    fs::write(src.join("b.svg"), TEST_SVG).unwrap();

    let out = dir.path().join("out");

    let output = svgm()
        .args(["-r", src.to_str().unwrap()])
        .args(["-o", out.to_str().unwrap()])
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", lossy(&output.stderr));

    // Directory structure preserved in output
    assert!(
        out.join("sub/a.svg").exists(),
        "sub/a.svg should exist in output"
    );
    assert!(out.join("b.svg").exists(), "b.svg should exist in output");

    // Source files unchanged
    assert_eq!(fs::read_to_string(src.join("sub/a.svg")).unwrap(), TEST_SVG);
    assert_eq!(fs::read_to_string(src.join("b.svg")).unwrap(), TEST_SVG);
}

#[test]
fn recursive_multiple_inputs_errors() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    let file = write_svg(&dir, "a.svg");

    let output = svgm()
        .args(["-r", sub.to_str().unwrap()])
        .arg(&file)
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(lossy(&output.stderr).contains("exactly one directory"));
}

#[test]
fn recursive_on_file_errors() {
    let dir = TempDir::new().unwrap();
    let file = write_svg(&dir, "a.svg");

    let output = svgm()
        .args(["-r", file.to_str().unwrap()])
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(lossy(&output.stderr).contains("is not a directory"));
}

#[test]
fn recursive_output_inside_source_errors() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.svg"), TEST_SVG).unwrap();

    let out = src.join("sub");

    let output = svgm()
        .args(["-r", src.to_str().unwrap()])
        .args(["-o", out.to_str().unwrap()])
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(lossy(&output.stderr).contains("must not overlap"));
}

#[test]
fn recursive_with_stdout_errors() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();

    let output = svgm()
        .args(["-r", sub.to_str().unwrap()])
        .arg("--stdout")
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(lossy(&output.stderr).contains("cannot use --stdout with -r"));
}

#[test]
fn directory_without_recursive_errors() {
    let dir = TempDir::new().unwrap();

    let output = svgm().arg(dir.path()).arg("--no-config").output().unwrap();
    assert!(!output.status.success());
    assert!(lossy(&output.stderr).contains("use -r to process recursively"));
}

#[test]
fn recursive_empty_directory() {
    let dir = TempDir::new().unwrap();

    let output = svgm()
        .args(["-r", dir.path().to_str().unwrap()])
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(lossy(&output.stderr).contains("0 SVG files found"));
}

// ── File filtering ─────────────────────────────────────────────────────

#[test]
fn recursive_skips_non_svg() {
    let dir = TempDir::new().unwrap();
    write_svg(&dir, "a.svg");
    let txt = dir.path().join("b.txt");
    fs::write(&txt, "not an svg").unwrap();

    let output = svgm()
        .args(["-r", dir.path().to_str().unwrap()])
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(output.status.success());

    assert_eq!(
        fs::read_to_string(&txt).unwrap(),
        "not an svg",
        "non-SVG file should not be touched"
    );
}

#[cfg(unix)]
#[test]
fn recursive_skips_symlinks() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let real = write_svg(&dir, "real.svg");
    symlink(&real, dir.path().join("link.svg")).unwrap();

    let output = svgm()
        .args(["-r", dir.path().to_str().unwrap()])
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", lossy(&output.stderr));

    // stderr should mention "1 file" (the real one), not "2 files"
    let stderr = lossy(&output.stderr);
    assert!(
        stderr.contains("1 file"),
        "should process 1 file, not 2: {stderr}"
    );
}

// ── Multi-file + output ────────────────────────────────────────────────

#[test]
fn multiple_files_with_output_errors() {
    let dir = TempDir::new().unwrap();
    let a = write_svg(&dir, "a.svg");
    let b = write_svg(&dir, "b.svg");
    let out = dir.path().join("out.svg");

    let output = svgm()
        .arg(&a)
        .arg(&b)
        .args(["-o", out.to_str().unwrap()])
        .arg("--no-config")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(lossy(&output.stderr).contains("cannot use -o with multiple"));
}

// ── Config discovery ───────────────────────────────────────────────────

#[test]
fn config_discovered_from_parent() {
    let dir = TempDir::new().unwrap();

    // Place config in parent directory
    fs::write(dir.path().join("svgm.config.toml"), "preset = \"safe\"\n").unwrap();

    // Place SVG in a subdirectory
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    let file = sub.join("a.svg");
    fs::write(&file, TEST_SVG).unwrap();

    // Use -o to force file output (pipe detection would write to stdout otherwise).
    let output = svgm()
        .arg(&file)
        .args(["-o", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "config should be discovered from parent: {}",
        lossy(&output.stderr)
    );

    // File should be optimized (safe preset still removes comments, metadata, etc.)
    let result = fs::read_to_string(&file).unwrap();
    assert_ne!(
        result, TEST_SVG,
        "file should be optimized with discovered config"
    );
    assert!(
        !result.contains("<!--"),
        "safe preset should remove comments"
    );
}

// ── Helper ─────────────────────────────────────────────────────────────

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}
