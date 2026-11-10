//! MONTI core: deterministic Tree-sitter detection of LLM slop.

pub mod ast;
pub mod detect;
pub mod index;
pub mod languages;
pub mod metrics;
pub mod rules;

use anyhow::{Context, Result};
use ast::Language;
use detect::Hit;
use ignore::WalkBuilder;
use metrics::Metrics;
use rules::{Check, Rule, Severity};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, QueryCursor};

/// Extra ignore file, gitignore syntax, for paths to keep out of scans (fixtures, vendored code).
pub const IGNORE_FILE: &str = ".montiignore";

/// Replace `src[start..end]` (byte offsets) with `replacement`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub rule_id: String,
    pub severity: Severity,
    pub message: String,
    /// 1-based.
    pub line: usize,
    /// 1-based.
    pub column: usize,
    pub fix: Option<Edit>,
}

impl Finding {
    fn new(rule: &Rule, line: usize, column: usize, message: String, fix: Option<Edit>) -> Finding {
        Finding { rule_id: rule.id.clone(), severity: rule.severity, message, line, column, fix }
    }
}

#[derive(Debug)]
pub struct FileReport {
    pub path: PathBuf,
    pub language: Language,
    pub metrics: Metrics,
    pub findings: Vec<Finding>,
}

impl FileReport {
    /// Weighted findings (error 3, warning 1) per 100 code lines.
    pub fn slop_score(&self) -> f64 {
        let weight: usize = self
            .findings
            .iter()
            .map(|f| match f.severity {
                Severity::Error => 3,
                Severity::Warning => 1,
            })
            .sum();
        weight as f64 * 100.0 / self.metrics.code_lines.max(1) as f64
    }
}

/// Runs the per-file rules that apply to `lang` against `source`. Repo-wide rules
/// (duplicates, ghost abstractions) need [`scan_path`].
pub fn scan(lang: Language, source: &str, rules: &[Rule]) -> Result<Vec<Finding>> {
    let tree = ast::parse(lang, source)?;
    let mut findings = scan_tree(lang, tree.root_node(), source, rules);
    findings.sort_by_key(|f| (f.line, f.column));
    Ok(findings)
}

fn scan_tree(lang: Language, root: Node, src: &str, rules: &[Rule]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for rule in rules.iter().filter(|r| r.languages.contains(&lang)) {
        let hits: Vec<Hit> = match &rule.check {
            Check::Query(queries) => {
                let mut cursor = QueryCursor::new();
                let mut matches = cursor.matches(&queries[&lang], root, src.as_bytes());
                let mut hits = Vec::new();
                while let Some(m) = matches.next() {
                    let node = m.captures().iter().map(|c| c.node).min_by_key(|n| n.start_byte()).unwrap();
                    hits.push(Hit { node, fix: None });
                }
                hits
            }
            Check::MaxCommentCodeRatio(max) => metrics::functions(lang, root)
                .into_iter()
                .filter(|f| metrics::comment_code_ratio(*f, src) > *max)
                .map(|node| Hit { node, fix: None })
                .collect(),
            Check::PassThroughWrapper => detect::pass_through_wrappers(lang, root, src),
            Check::RedundantNullCheck => detect::redundant_null_checks(lang, root, src),
            Check::RestatingComment => detect::restating_comments(root, src),
            Check::DuplicateFunction { .. } | Check::GhostAbstraction => continue,
        };
        findings.extend(hits.into_iter().map(|h| {
            let p = h.node.start_position();
            Finding::new(rule, p.row + 1, p.column + 1, rule.message.clone(), h.fix)
        }));
    }
    findings
}

/// Supported source files under `root` (or `root` itself), honoring .gitignore and .montiignore, sorted.
pub fn source_files(root: &Path) -> Result<Vec<(PathBuf, Language)>> {
    let mut out = Vec::new();
    for entry in WalkBuilder::new(root).add_custom_ignore_filename(IGNORE_FILE).build() {
        let entry = entry?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if let Some(lang) = Language::from_path(entry.path()) {
            out.push((entry.into_path(), lang));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// A file the scan could not analyse, and why.
#[derive(Debug, Clone, Serialize)]
pub struct Skipped {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug)]
pub struct Scan {
    pub files: Vec<FileReport>,
    pub skipped: Vec<Skipped>,
}

pub(crate) struct Source {
    pub path: PathBuf,
    pub language: Language,
    pub text: String,
    pub tree: tree_sitter::Tree,
}

/// Reads and parses every supported file under `root`. Files that are not UTF-8, look minified,
/// or do not parse are skipped with a reason: rules on an error-recovered tree produce noise, and
/// grammars lag the languages, so one unparseable file must not stop the scan.
pub(crate) fn load_sources(root: &Path) -> Result<(Vec<Source>, Vec<Skipped>)> {
    let loaded = par_map(&source_files(root)?, |(path, language)| -> Result<Result<Source, Skipped>> {
        let (path, language) = (path.clone(), *language);
        let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let parsed = String::from_utf8(bytes)
            .map_err(|_| "not UTF-8".to_string())
            .and_then(|text| if is_minified(&text) { Err("minified".to_string()) } else { Ok(text) })
            .and_then(|text| match ast::parse(language, &text) {
                Ok(tree) => Ok((text, tree)),
                Err(e) => Err(format!("{e:#}")),
            });
        Ok(match parsed {
            Ok((text, tree)) => Ok(Source { path, language, text, tree }),
            Err(reason) => Err(Skipped { path, reason }),
        })
    });
    let mut sources = Vec::new();
    let mut skipped = Vec::new();
    for item in loaded {
        match item? {
            Ok(source) => sources.push(source),
            Err(skip) => skipped.push(skip),
        }
    }
    Ok((sources, skipped))
}

/// `items.iter().map(f)` on all cores, results in input order.
pub(crate) fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut done: Vec<(usize, R)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(i) else { return mine };
                        mine.push((i, f(item)));
                    }
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().expect("scan worker panicked")).collect()
    });
    done.sort_unstable_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, r)| r).collect()
}

/// Bundled or minified code averages hundreds of characters per line; hand-written code ~40.
fn is_minified(text: &str) -> bool {
    let lines = text.lines().filter(|l| !l.trim().is_empty()).count();
    lines > 0 && text.len() / lines > 300
}

/// Scans every supported file under `root` with per-file and repo-wide rules.
pub fn scan_path(root: &Path, rules: &[Rule]) -> Result<Scan> {
    let (sources, skipped) = load_sources(root)?;
    let analysed = par_map(&sources, |s| {
        let root_node = s.tree.root_node();
        let report = FileReport {
            findings: scan_tree(s.language, root_node, &s.text, rules),
            metrics: metrics::measure(root_node, &s.text),
            path: s.path.clone(),
            language: s.language,
        };
        (report, index::Index::of_file(&s.path, s.language, root_node, &s.text))
    });
    let mut index = index::Index::default();
    let mut reports = Vec::new();
    for (report, part) in analysed {
        reports.push(report);
        index.merge(part);
    }

    let slot: HashMap<PathBuf, usize> = reports.iter().enumerate().map(|(i, r)| (r.path.clone(), i)).collect();
    for rule in rules {
        let mut push = |path: &Path, line, column, message| {
            reports[slot[path]].findings.push(Finding::new(rule, line, column, message, None));
        };
        match rule.check {
            Check::DuplicateFunction { min_similarity } => {
                for (a, b, s) in index.duplicates(min_similarity) {
                    if !rule.languages.contains(&a.language) {
                        continue;
                    }
                    for (this, other) in [(a, b), (b, a)] {
                        let message = format!(
                            "{} ({:.0}% match with `{}` at {}:{})",
                            rule.message,
                            s * 100.0,
                            other.name,
                            other.path.display(),
                            other.line
                        );
                        push(&this.path, this.line, this.column, message);
                    }
                }
            }
            Check::GhostAbstraction => {
                for g in index.ghosts().into_iter().filter(|g| rule.languages.contains(&g.language)) {
                    push(&g.path, g.line, g.column, format!("{} (`{}`)", rule.message, g.name));
                }
            }
            _ => {}
        }
    }

    for r in &mut reports {
        r.findings.sort_by(|a, b| (a.line, a.column, &a.rule_id, &a.message).cmp(&(b.line, b.column, &b.rule_id, &b.message)));
    }
    Ok(Scan { files: reports, skipped })
}

/// Applies edits to `src`. An edit overlapping an earlier one is skipped; rerun to pick it up.
pub fn apply_fixes(src: &str, edits: &[&Edit]) -> String {
    let mut edits = edits.to_vec();
    edits.sort_by_key(|e| e.start);
    let mut out = String::with_capacity(src.len());
    let mut pos = 0;
    for e in edits {
        if e.start < pos {
            continue;
        }
        out.push_str(&src[pos..e.start]);
        out.push_str(&e.replacement);
        pos = e.end;
    }
    out.push_str(&src[pos..]);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn par_map_keeps_input_order() {
        let items: Vec<u64> = (0..1000).collect();
        assert_eq!(super::par_map(&items, |x| x * 2), items.iter().map(|x| x * 2).collect::<Vec<_>>());
    }
}
