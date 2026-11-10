//! Which findings a scan reports: only changed lines (`--since`), minus the accepted baseline.

use anyhow::{Context, Result, ensure};
use monti_core::{FileReport, Finding};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const BASELINE_FILE: &str = ".monti-baseline.json";

/// The directory a scan of `path` is rooted at: the path itself, or a file's parent.
pub fn root_dir(path: &Path) -> &Path {
    if path.is_file() { path.parent().unwrap_or(Path::new(".")) } else { path }
}

fn relative(root: &Path, file: &Path) -> PathBuf {
    file.strip_prefix(root).unwrap_or(file).to_path_buf()
}

/// Keeps findings on lines added or changed since git `base` (committed or not), and every
/// finding in untracked files.
// shortcut: a repo-wide finding (duplicate, ghost) is kept only when its own line changed; widen to
// the whole function if users miss duplicates introduced by body edits.
pub fn keep_changed(root: &Path, base: &str, files: &mut [FileReport]) -> Result<()> {
    let git = |args: &[&str]| -> Result<String> {
        let out = Command::new("git").args(args).current_dir(root).output().context("running git")?;
        ensure!(out.status.success(), "git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
        Ok(String::from_utf8(out.stdout)?)
    };
    let diff = git(&["diff", "--unified=0", "--no-color", "--no-ext-diff", "--relative", base, "--", "."])?;
    let changed = changed_lines(&diff);
    let untracked: HashSet<PathBuf> = git(&["ls-files", "--others", "--exclude-standard"])?.lines().map(PathBuf::from).collect();
    for r in files {
        let rel = relative(root, &r.path);
        if untracked.contains(&rel) {
            continue;
        }
        let lines = changed.get(&rel);
        r.findings.retain(|f| lines.is_some_and(|l| l.contains(&f.line)));
    }
    Ok(())
}

/// New-side line numbers per file from `git diff --unified=0`.
fn changed_lines(diff: &str) -> HashMap<PathBuf, HashSet<usize>> {
    let mut out: HashMap<PathBuf, HashSet<usize>> = HashMap::new();
    let mut current: Option<PathBuf> = None;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            current = path.strip_prefix("b/").map(PathBuf::from);
        } else if let (Some(hunk), Some(path)) = (line.strip_prefix("@@ "), &current) {
            // @@ -a,b +c,d @@  ->  lines c .. c+d (d defaults to 1)
            let Some(new) = hunk.split(' ').find_map(|p| p.strip_prefix('+')) else { continue };
            let mut it = new.split(',').map(|n| n.parse::<usize>().unwrap_or(0));
            let start = it.next().unwrap_or(0);
            let count = it.next().unwrap_or(1);
            out.entry(path.clone()).or_default().extend(start..start + count);
        }
    }
    out
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
struct Entry {
    path: PathBuf,
    rule: String,
    fingerprint: String,
}

#[derive(Serialize, Deserialize)]
struct Baseline {
    version: u32,
    findings: Vec<Entry>,
}

fn entry(root: &Path, file: &Path, f: &Finding) -> Entry {
    Entry { path: relative(root, file), rule: f.rule_id.clone(), fingerprint: f.fingerprint.clone() }
}

/// Writes every current finding to `<root>/.monti-baseline.json`. Returns how many.
pub fn write_baseline(root: &Path, files: &[FileReport]) -> Result<usize> {
    let findings: Vec<Entry> = files.iter().flat_map(|r| r.findings.iter().map(|f| entry(root, &r.path, f))).collect();
    let n = findings.len();
    fs::write(root.join(BASELINE_FILE), serde_json::to_string_pretty(&Baseline { version: 1, findings })? + "\n")?;
    Ok(n)
}

/// Drops findings recorded in `<root>/.monti-baseline.json`, if it exists. Each entry absorbs one finding.
pub fn apply_baseline(root: &Path, files: &mut [FileReport]) -> Result<()> {
    let path = root.join(BASELINE_FILE);
    if !path.exists() {
        return Ok(());
    }
    let baseline: Baseline = serde_json::from_str(&fs::read_to_string(&path)?).with_context(|| format!("reading {}", path.display()))?;
    ensure!(baseline.version == 1, "{}: unsupported version {}", path.display(), baseline.version);
    let mut accepted: HashMap<Entry, usize> = HashMap::new();
    for e in baseline.findings {
        *accepted.entry(e).or_default() += 1;
    }
    for r in files {
        let file = r.path.clone();
        r.findings.retain(|f| match accepted.get_mut(&entry(root, &file, f)) {
            Some(n) if *n > 0 => {
                *n -= 1;
                false
            }
            _ => true,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn changed_lines_reads_hunks() {
        let diff = "diff --git a/x.py b/x.py\n--- a/x.py\n+++ b/x.py\n@@ -3 +3 @@\n-a\n+b\n@@ -10,0 +11,2 @@\n+c\n+d\n@@ -20,2 +22,0 @@\n-e\n-f\n+++ /dev/null\n";
        let got = super::changed_lines(diff);
        let mut lines: Vec<usize> = got[std::path::Path::new("x.py")].iter().copied().collect();
        lines.sort();
        assert_eq!(lines, [3, 11, 12]);
    }
}
