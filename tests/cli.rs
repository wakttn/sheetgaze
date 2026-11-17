use assert_cmd::Command;
use predicates::prelude::*;

fn cmd() -> Command {
    Command::cargo_bin("sheetgaze").unwrap()
}

fn csv_fixture(contents: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.csv");
    std::fs::write(&path, contents).unwrap();
    (dir, path)
}

#[test]
fn help_and_version_work() {
    cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("max-col-width"));
    cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("sheetgaze"));
}

#[test]
fn invalid_option_value_exits_two() {
    cmd()
        .args(["--max-col-width", "1", "example.csv"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("max-col-width"));
}

#[test]
fn missing_file_exits_one() {
    cmd()
        .arg("/definitely/not/a/file.csv")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("csv error"));
}

#[test]
fn print_mode_renders_plain_snapshot() {
    let (_dir, path) = csv_fixture("Name,Value\nEast,10\nWest,20\n");
    let output = cmd()
        .args(["--print", "--no-ansi", "--width", "60", "--height", "12"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("sample.csv"),
        "status line missing: {stdout}"
    );
    assert!(stdout.contains("Name"), "grid header missing: {stdout}");
    assert!(stdout.contains("East"), "cell value missing: {stdout}");
    assert!(
        !stdout.contains('\u{1b}'),
        "unexpected ANSI escape: {stdout}"
    );
    assert_eq!(stdout.lines().count(), 12);
}

#[test]
fn print_mode_applies_fit_width() {
    let mut text = String::new();
    for col in 0..10 {
        if col > 0 {
            text.push(',');
        }
        text.push_str(&format!("column-{col:02}"));
    }
    text.push('\n');
    for col in 0..10 {
        if col > 0 {
            text.push(',');
        }
        text.push('1');
    }
    text.push('\n');
    let (_dir, path) = csv_fixture(&text);

    let without = cmd()
        .args(["--print", "--no-ansi", "--width", "40", "--height", "10"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(without.status.success());
    let without = String::from_utf8(without.stdout).unwrap();
    assert!(!without.contains("fit:on"));

    let with = cmd()
        .args([
            "--print",
            "--no-ansi",
            "--width",
            "40",
            "--height",
            "10",
            "--fit-width",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(with.status.success());
    let with = String::from_utf8(with.stdout).unwrap();
    assert!(with.contains("fit:on"), "fit label missing: {with}");
}

#[test]
fn print_mode_emits_ansi_by_default() {
    let (_dir, path) = csv_fixture("Name,Value\nEast,10\n");
    let output = cmd()
        .args(["--print", "--width", "60", "--height", "12"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains('\u{1b}'), "ANSI escape missing");
    assert!(stdout.contains("\u{1b}[0;"));
    assert!(stdout.contains("East"));
}

#[test]
fn print_mode_respects_sheet_selection() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/xlsx/multi_sheet.xlsx");
    let output = cmd()
        .args([
            "--print",
            "--no-ansi",
            "--width",
            "60",
            "--height",
            "12",
            "--sheet",
        ])
        .arg("データ")
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("データ"));
    assert!(stdout.contains("東京"));
}

#[test]
fn print_mode_omits_sheet_tabs() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/xlsx/multi_sheet.xlsx");
    let output = cmd()
        .args([
            "--print",
            "--no-ansi",
            "--width",
            "60",
            "--height",
            "12",
            "--sheet",
        ])
        .arg("データ")
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("[データ]"), "status line missing: {stdout}");
    assert!(
        !stdout.lines().any(|line| line.contains("空シート")),
        "inactive sheet tab should not be printed: {stdout}"
    );
    let last = stdout.lines().last().unwrap_or_default();
    assert_ne!(
        last.trim(),
        "[データ]",
        "active sheet tab should be omitted"
    );
}

#[test]
fn print_size_must_be_in_range() {
    let (_dir, path) = csv_fixture("a,b\n1,2\n");
    cmd()
        .args(["--print", "--width", "20"])
        .arg(&path)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--width"));
}

#[test]
fn print_options_require_print_mode() {
    let (_dir, path) = csv_fixture("a,b\n1,2\n");
    cmd()
        .args(["--width", "60"])
        .arg(&path)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--print"));
}
