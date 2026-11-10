//! Report formats.

use monti_core::rules::{Rule, Severity};
use monti_core::{FileReport, Scan};
use serde_json::{Value, json};
use std::io::IsTerminal;
use std::path::Path;

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Format {
    /// Human-readable table.
    Text,
    /// Everything, for scripts.
    Json,
    /// SARIF 2.1.0, for GitHub code scanning and other viewers.
    Sarif,
    /// GitHub Actions workflow commands: inline annotations on the PR.
    Github,
}

fn shown(path: &Path) -> String {
    let p = path.strip_prefix(".").unwrap_or(path);
    p.to_string_lossy().replace('\\', "/")
}

pub fn render(format: Format, scan: &Scan, rules: &[Rule]) -> String {
    match format {
        Format::Text => text(scan),
        Format::Json => json(scan).to_string(),
        Format::Sarif => serde_json::to_string_pretty(&sarif(scan, rules)).expect("SARIF serializes"),
        Format::Github => github(scan),
    }
}

fn counts(files: &[FileReport]) -> (usize, usize) {
    let all = files.iter().flat_map(|r| &r.findings);
    let errors = all.clone().filter(|f| f.severity == Severity::Error).count();
    (errors, all.count() - errors)
}

fn text(scan: &Scan) -> String {
    let color = std::io::stdout().is_terminal();
    let paint = |code: &str, s: &str| if color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() };
    let mut out = String::new();
    for r in scan.files.iter().filter(|r| !r.findings.is_empty()) {
        out += &format!("{}  slop score {:.1}\n", paint("1", &shown(&r.path)), r.slop_score());
        for f in &r.findings {
            let severity = match f.severity {
                Severity::Error => paint("31", "error  "),
                Severity::Warning => paint("33", "warning"),
            };
            out += &format!("  {:>5}:{:<4} {severity}  {:<22} {}\n", f.line, f.column, f.rule_id, f.message);
        }
        out += "\n";
    }
    for s in &scan.skipped {
        out += &format!("{}  {}\n", paint("2", &format!("skipped {}", shown(&s.path))), s.reason);
    }
    let (errors, warnings) = counts(&scan.files);
    out + &format!("{} files scanned, {} skipped, {errors} errors, {warnings} warnings\n", scan.files.len(), scan.skipped.len())
}

fn json(scan: &Scan) -> Value {
    let (errors, warnings) = counts(&scan.files);
    json!({
        "files": scan.files.iter().map(|r| json!({
            "path": shown(&r.path),
            "language": r.language,
            "slop_score": r.slop_score(),
            "findings": r.findings,
        })).collect::<Vec<_>>(),
        "skipped": scan.skipped.iter().map(|s| json!({ "path": shown(&s.path), "reason": s.reason })).collect::<Vec<_>>(),
        "errors": errors,
        "warnings": warnings,
    })
}

fn sarif(scan: &Scan, rules: &[Rule]) -> Value {
    let results: Vec<Value> = scan
        .files
        .iter()
        .flat_map(|r| r.findings.iter().map(move |f| (r, f)))
        .map(|(r, f)| {
            json!({
                "ruleId": f.rule_id,
                "level": match f.severity { Severity::Error => "error", Severity::Warning => "warning" },
                "message": { "text": f.message },
                "locations": [{ "physicalLocation": {
                    "artifactLocation": { "uri": shown(&r.path) },
                    "region": { "startLine": f.line, "startColumn": f.column }
                }}],
                "partialFingerprints": { "monti/v1": f.fingerprint },
            })
        })
        .collect();
    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": { "driver": {
                "name": "monti",
                "informationUri": "https://github.com/JFrusher/MONTI",
                "version": env!("CARGO_PKG_VERSION"),
                "rules": rules.iter().map(|r| json!({ "id": r.id, "shortDescription": { "text": r.message } })).collect::<Vec<_>>(),
            }},
            "results": results,
        }]
    })
}

/// https://docs.github.com/actions/reference/workflow-commands-for-github-actions
fn github(scan: &Scan) -> String {
    let data = |s: &str| s.replace('%', "%25").replace('\r', "%0D").replace('\n', "%0A");
    let prop = |s: &str| data(s).replace(':', "%3A").replace(',', "%2C");
    let mut out = String::new();
    for r in &scan.files {
        for f in &r.findings {
            let level = match f.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            out += &format!(
                "::{level} file={},line={},col={},title={}::{}\n",
                prop(&shown(&r.path)),
                f.line,
                f.column,
                prop(&f.rule_id),
                data(&f.message)
            );
        }
    }
    let (errors, warnings) = counts(&scan.files);
    out + &format!("monti: {} files scanned, {} skipped, {errors} errors, {warnings} warnings\n", scan.files.len(), scan.skipped.len())
}

pub fn has_errors(scan: &Scan) -> bool {
    counts(&scan.files).0 > 0
}
