use crate::ast::Language;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use tree_sitter::Query;

pub const DEFAULT_RULES: &str = include_str!("../../../rules/default.toml");
pub const CONFIG_FILE: &str = "monti.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Warning,
    Error,
}

/// Detectors written in Rust because a query cannot express them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Builtin {
    PassThroughWrapper,
    RedundantNullCheck,
    RestatingComment,
    DuplicateFunction,
    GhostAbstraction,
}

impl Builtin {
    fn languages(self) -> &'static [Language] {
        use Language::*;
        match self {
            Builtin::PassThroughWrapper | Builtin::RestatingComment | Builtin::DuplicateFunction => {
                &[TypeScript, Tsx, JavaScript, Python, Rust, Go]
            }
            Builtin::RedundantNullCheck => &[TypeScript, Tsx, JavaScript, Python, Go],
            // Go interfaces are satisfied implicitly, so implementors cannot be counted syntactically.
            // JavaScript has no interfaces or abstract classes.
            Builtin::GhostAbstraction => &[TypeScript, Tsx, Python, Rust],
        }
    }
}

#[derive(Debug)]
pub enum Check {
    /// One compiled query per language the rule targets. Findings point at the earliest capture.
    Query(HashMap<Language, Query>),
    /// Flags functions whose comment lines / code lines exceed the limit.
    MaxCommentCodeRatio(f64),
    PassThroughWrapper,
    RedundantNullCheck,
    RestatingComment,
    /// Repo-wide: functions whose normalized AST shingles overlap by at least this Jaccard similarity.
    DuplicateFunction { min_similarity: f64 },
    /// Repo-wide: an interface/abstract class/trait with one implementor and at most one other use.
    GhostAbstraction,
}

#[derive(Debug)]
pub struct Rule {
    pub id: String,
    pub severity: Severity,
    pub message: String,
    pub languages: Vec<Language>,
    pub check: Check,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleFile {
    rules: BTreeMap<String, RawRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    severity: Severity,
    message: String,
    languages: Vec<Language>,
    tree_sitter_query: Option<String>,
    max_comment_code_ratio: Option<f64>,
    check: Option<Builtin>,
    min_similarity: Option<f64>,
}

/// `<dir>/monti.toml` if present (it replaces the defaults), else the embedded defaults.
/// `path` may be a file; its directory is used.
pub fn load(path: &Path) -> Result<Vec<Rule>> {
    let dir = if path.is_file() { path.parent().context("file has no parent")? } else { path };
    let config = dir.join(CONFIG_FILE);
    if !config.exists() {
        return parse_rules(DEFAULT_RULES);
    }
    let src = fs::read_to_string(&config).with_context(|| format!("reading {}", config.display()))?;
    parse_rules(&src).with_context(|| format!("loading {}", config.display()))
}

/// Parses a rule file. The `[rules.<id>]` table key is the rule id.
/// Every query is compiled here so a bad rule fails at load, not mid-scan.
pub fn parse_rules(toml_src: &str) -> Result<Vec<Rule>> {
    let file: RuleFile = toml::from_str(toml_src)?;
    file.rules.into_iter().map(|(id, raw)| compile(id, raw)).collect()
}

fn compile(id: String, raw: RawRule) -> Result<Rule> {
    ensure!(!raw.languages.is_empty(), "rule `{id}`: `languages` is empty");
    ensure!(
        raw.min_similarity.is_none() || raw.check == Some(Builtin::DuplicateFunction),
        "rule `{id}`: `min_similarity` only applies to check = \"duplicate-function\""
    );
    let check = match (raw.tree_sitter_query, raw.max_comment_code_ratio, raw.check) {
        (Some(src), None, None) => {
            let mut queries = HashMap::new();
            for &lang in &raw.languages {
                let query = Query::new(&lang.grammar(), &src)
                    .with_context(|| format!("rule `{id}`: invalid query for {lang:?}"))?;
                ensure!(!query.capture_names().is_empty(), "rule `{id}`: query has no capture to report");
                queries.insert(lang, query);
            }
            Check::Query(queries)
        }
        (None, Some(max), None) => {
            ensure!(max >= 0.0, "rule `{id}`: max_comment_code_ratio must be >= 0");
            Check::MaxCommentCodeRatio(max)
        }
        (None, None, Some(builtin)) => {
            for lang in &raw.languages {
                ensure!(builtin.languages().contains(lang), "rule `{id}`: {builtin:?} does not support {lang:?}");
            }
            match builtin {
                Builtin::PassThroughWrapper => Check::PassThroughWrapper,
                Builtin::RedundantNullCheck => Check::RedundantNullCheck,
                Builtin::RestatingComment => Check::RestatingComment,
                Builtin::GhostAbstraction => Check::GhostAbstraction,
                Builtin::DuplicateFunction => {
                    let min = raw.min_similarity.with_context(|| format!("rule `{id}`: needs `min_similarity`"))?;
                    ensure!(min > 0.0 && min <= 1.0, "rule `{id}`: min_similarity must be in (0, 1]");
                    Check::DuplicateFunction { min_similarity: min }
                }
            }
        }
        _ => bail!("rule `{id}`: set exactly one of `tree_sitter_query`, `max_comment_code_ratio` or `check`"),
    };
    Ok(Rule {
        id,
        severity: raw.severity,
        message: raw.message,
        languages: raw.languages,
        check,
    })
}
