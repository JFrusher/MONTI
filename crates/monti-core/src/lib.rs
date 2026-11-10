//! MONTI core: deterministic Tree-sitter detection of LLM slop.

pub mod ast;
pub mod metrics;
pub mod rules;

use anyhow::Result;
use ast::Language;
use rules::{Check, Rule, Severity};
use streaming_iterator::StreamingIterator;
use tree_sitter::QueryCursor;

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub rule_id: String,
    pub severity: Severity,
    pub message: String,
    /// 1-based.
    pub line: usize,
    /// 1-based.
    pub column: usize,
}

/// Runs every rule that applies to `lang` against `source`. Findings are sorted by position.
pub fn scan(lang: Language, source: &str, rules: &[Rule]) -> Result<Vec<Finding>> {
    let tree = ast::parse(lang, source)?;
    let root = tree.root_node();
    let mut findings = Vec::new();

    for rule in rules.iter().filter(|r| r.languages.contains(&lang)) {
        let mut report = |node: tree_sitter::Node| {
            let p = node.start_position();
            findings.push(Finding {
                rule_id: rule.id.clone(),
                severity: rule.severity,
                message: rule.message.clone(),
                line: p.row + 1,
                column: p.column + 1,
            });
        };
        match &rule.check {
            Check::Query(queries) => {
                let query = &queries[&lang];
                let mut cursor = QueryCursor::new();
                let mut matches = cursor.matches(query, root, source.as_bytes());
                while let Some(m) = matches.next() {
                    report(m.captures()[0].node);
                }
            }
            Check::MaxCommentCodeRatio(max) => {
                for func in metrics::functions(lang, root) {
                    if metrics::comment_code_ratio(func) > *max {
                        report(func);
                    }
                }
            }
        }
    }

    findings.sort_by_key(|f| (f.line, f.column));
    Ok(findings)
}
