use crate::ast::Language;
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use tree_sitter::Query;

pub const DEFAULT_RULES: &str = include_str!("../../../rules/default.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug)]
pub enum Check {
    /// One compiled query per language the rule targets. Findings point at the first capture.
    Query(HashMap<Language, Query>),
    /// Flags functions whose comment lines / code lines exceed the limit.
    MaxCommentCodeRatio(f64),
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
}

/// Parses a rule file. The `[rules.<id>]` table key is the rule id.
/// Every query is compiled here so a bad rule fails at load, not mid-scan.
pub fn parse_rules(toml_src: &str) -> Result<Vec<Rule>> {
    let file: RuleFile = toml::from_str(toml_src)?;
    file.rules.into_iter().map(|(id, raw)| compile(id, raw)).collect()
}

fn compile(id: String, raw: RawRule) -> Result<Rule> {
    ensure!(!raw.languages.is_empty(), "rule `{id}`: `languages` is empty");
    let check = match (raw.tree_sitter_query, raw.max_comment_code_ratio) {
        (Some(src), None) => {
            let mut queries = HashMap::new();
            for &lang in &raw.languages {
                let query = Query::new(&lang.grammar(), &src)
                    .with_context(|| format!("rule `{id}`: invalid query for {lang:?}"))?;
                ensure!(!query.capture_names().is_empty(), "rule `{id}`: query has no capture to report");
                queries.insert(lang, query);
            }
            Check::Query(queries)
        }
        (None, Some(max)) => {
            ensure!(max >= 0.0, "rule `{id}`: max_comment_code_ratio must be >= 0");
            Check::MaxCommentCodeRatio(max)
        }
        _ => bail!("rule `{id}`: set exactly one of `tree_sitter_query` or `max_comment_code_ratio`"),
    };
    Ok(Rule {
        id,
        severity: raw.severity,
        message: raw.message,
        languages: raw.languages,
        check,
    })
}
