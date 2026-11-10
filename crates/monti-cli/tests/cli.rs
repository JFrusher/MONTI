use std::fs;
use std::process::Command;

const MONTI: &str = env!("CARGO_BIN_EXE_monti");

#[path = "../../monti-core/tests/common/mod.rs"]
mod common;
use common::temp_dir;

const SLOP: &str = "def build():\n    items = []\n    if items is not None:\n        items.append(1)\n    return items\n\n\ndef load(p):\n    try:\n        return open(p).read()\n    except OSError:\n        pass\n";

#[test]
fn scan_reports_and_fails_on_errors() {
    let dir = temp_dir("scan");
    fs::write(dir.join("a.py"), SLOP).unwrap();
    let out = Command::new(MONTI).arg("scan").arg(&dir).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("swallowed-error"), "{stdout}");
    assert!(stdout.contains("1 files scanned, 0 skipped, 1 errors, 1 warnings"), "{stdout}");
}

#[test]
fn fix_rewrites_and_init_writes_config() {
    let dir = temp_dir("fix");
    fs::write(dir.join("a.py"), SLOP).unwrap();
    let out = Command::new(MONTI).args(["fix", "--no-format", "--no-verify"]).arg(&dir).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let fixed = fs::read_to_string(dir.join("a.py")).unwrap();
    assert!(fixed.starts_with("def build():\n    items = []\n    items.append(1)\n"), "{fixed}");

    let out = Command::new(MONTI).args(["fix", "--no-format"]).arg(&dir).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("nothing to fix"));

    assert!(Command::new(MONTI).arg("init").arg(&dir).status().unwrap().success());
    assert!(fs::read_to_string(dir.join("monti.toml")).unwrap().contains("[rules.swallowed-error]"));
    assert!(!Command::new(MONTI).arg("init").arg(&dir).status().unwrap().success(), "init must not overwrite");
}

#[test]
fn fix_refuses_without_test_suite() {
    let dir = temp_dir("verify");
    fs::write(dir.join("a.py"), SLOP).unwrap();
    let out = Command::new(MONTI).args(["fix", "--no-format"]).arg(&dir).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no test suite found"));
    assert_eq!(fs::read_to_string(dir.join("a.py")).unwrap(), SLOP);
}
