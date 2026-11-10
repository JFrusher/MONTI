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
    // `safe` only deletes comments; SLOP has none, so the null check survives.
    let out = Command::new(MONTI).args(["fix", "--level", "safe", "--no-format", "--no-verify"]).arg(&dir).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("nothing to fix"));
    assert_eq!(fs::read_to_string(dir.join("a.py")).unwrap(), SLOP);
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

fn git(dir: &std::path::Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

const SWALLOW: &str = "def f():\n    try:\n        print()\n    except E:\n        pass\n";

#[test]
fn since_reports_only_changed_lines() {
    let dir = temp_dir("since");
    fs::write(dir.join("old.py"), SWALLOW).unwrap();
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "base"]);
    // Old slop stays, new slop is appended to the tracked file and added in an untracked one.
    fs::write(dir.join("old.py"), format!("{SWALLOW}\n\n{}", SWALLOW.replace("def f", "def g"))).unwrap();
    fs::write(dir.join("new.py"), SWALLOW).unwrap();
    let out = Command::new(MONTI).args(["scan", "--format", "json", "--since", "HEAD"]).arg(&dir).output().unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let mut got: Vec<(String, u64)> = json["files"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|f| f["findings"].as_array().unwrap().iter().map(move |x| (f["path"].as_str().unwrap().rsplit('/').next().unwrap().to_string(), x["line"].as_u64().unwrap())))
        .collect();
    got.sort();
    assert_eq!(got, [("new.py".to_string(), 4), ("old.py".to_string(), 11)]);
}

#[test]
fn baseline_hides_accepted_findings_only() {
    let dir = temp_dir("baseline");
    fs::write(dir.join("a.py"), SWALLOW).unwrap();
    assert!(Command::new(MONTI).arg("baseline").arg(&dir).status().unwrap().success());
    let out = Command::new(MONTI).arg("scan").arg(&dir).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    // Moving the accepted code down does not resurrect it; new slop is still reported.
    fs::write(dir.join("a.py"), format!("import os\n\n\n{SWALLOW}\n\n{}", SWALLOW.replace("def f", "def g"))).unwrap();
    let out = Command::new(MONTI).args(["scan", "--format", "github"]).arg(&dir).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.lines().filter(|l| l.starts_with("::")).count(), 1, "{stdout}");
    assert!(stdout.contains("::error file="), "{stdout}");
    assert!(stdout.contains(",line=14,col=5,title=swallowed-error::"), "{stdout}");
}

#[test]
fn sarif_lists_rules_and_results() {
    let dir = temp_dir("sarif");
    fs::write(dir.join("a.py"), SWALLOW).unwrap();
    let out = Command::new(MONTI).args(["scan", "--format", "sarif"]).arg(&dir).output().unwrap();
    let sarif: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let run = &sarif["runs"][0];
    assert_eq!(sarif["version"], "2.1.0");
    assert!(run["tool"]["driver"]["rules"].as_array().unwrap().iter().any(|r| r["id"] == "swallowed-error"));
    let result = &run["results"][0];
    assert_eq!(result["ruleId"], "swallowed-error");
    assert_eq!(result["locations"][0]["physicalLocation"]["region"]["startLine"], 4);
    assert_eq!(result["partialFingerprints"]["monti/v1"].as_str().unwrap().len(), 16);
}
