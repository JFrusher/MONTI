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
    UndefinedName,
    UnusedParameter,
}

impl Builtin {
    fn supports(self, lang: Language) -> bool {
        let spec = lang.spec();
        match self {
            Builtin::PassThroughWrapper => !spec.calls.is_empty(),
            Builtin::RedundantNullCheck => spec.null_check.is_some(),
            Builtin::GhostAbstraction => spec.ghost.is_some(),
            Builtin::UndefinedName => spec.undefined.is_some(),
            Builtin::RestatingComment | Builtin::DuplicateFunction | Builtin::UnusedParameter => true,
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
    UndefinedName,
    UnusedParameter,
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
    /// For `check` and `max_comment_code_ratio`: language names, or `["*"]` for all that the check supports.
    languages: Option<Vec<String>>,
    /// Query rules: one Tree-sitter query per language name. The keys are the rule's languages.
    queries: Option<BTreeMap<String, String>>,
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
    ensure!(
        raw.min_similarity.is_none() || raw.check == Some(Builtin::DuplicateFunction),
        "rule `{id}`: `min_similarity` only applies to check = \"duplicate-function\""
    );
    let lookup = |name: &str| Language::named(name).with_context(|| format!("rule `{id}`: unknown language `{name}`"));
    let (languages, check) = match (raw.queries, raw.max_comment_code_ratio, raw.check) {
        (Some(sources), None, None) => {
            ensure!(raw.languages.is_none(), "rule `{id}`: query rules take their languages from `queries`; drop `languages`");
            ensure!(!sources.is_empty(), "rule `{id}`: `queries` is empty");
            let mut queries = HashMap::new();
            for (name, src) in &sources {
                let lang = lookup(name)?;
                let query = Query::new(&lang.grammar(), src)
                    .with_context(|| format!("rule `{id}`: invalid query for {name}"))?;
                ensure!(!query.capture_names().is_empty(), "rule `{id}`: {name} query has no capture to report");
                queries.insert(lang, query);
            }
            (queries.keys().copied().collect(), Check::Query(queries))
        }
        (None, Some(max), None) => {
            ensure!(max >= 0.0, "rule `{id}`: max_comment_code_ratio must be >= 0");
            (languages(&id, raw.languages, |_| true)?, Check::MaxCommentCodeRatio(max))
        }
        (None, None, Some(builtin)) => {
            let languages = languages(&id, raw.languages, |l| builtin.supports(l))?;
            for lang in &languages {
                ensure!(builtin.supports(*lang), "rule `{id}`: {builtin:?} does not support {lang:?}");
            }
            let check = match builtin {
                Builtin::PassThroughWrapper => Check::PassThroughWrapper,
                Builtin::RedundantNullCheck => Check::RedundantNullCheck,
                Builtin::RestatingComment => Check::RestatingComment,
                Builtin::GhostAbstraction => Check::GhostAbstraction,
                Builtin::UndefinedName => Check::UndefinedName,
                Builtin::UnusedParameter => Check::UnusedParameter,
                Builtin::DuplicateFunction => {
                    let min = raw.min_similarity.with_context(|| format!("rule `{id}`: needs `min_similarity`"))?;
                    ensure!(min > 0.0 && min <= 1.0, "rule `{id}`: min_similarity must be in (0, 1]");
                    Check::DuplicateFunction { min_similarity: min }
                }
            };
            (languages, check)
        }
        _ => bail!("rule `{id}`: set exactly one of `queries`, `max_comment_code_ratio` or `check`"),
    };
    Ok(Rule {
        id,
        severity: raw.severity,
        message: raw.message,
        languages,
        check,
    })
}

/// Resolves `languages`; `["*"]` means every language for which `supported` holds.
fn languages(id: &str, names: Option<Vec<String>>, supported: impl Fn(Language) -> bool) -> Result<Vec<Language>> {
    let names = names.with_context(|| format!("rule `{id}`: needs `languages`"))?;
    ensure!(!names.is_empty(), "rule `{id}`: `languages` is empty");
    if names == ["*"] {
        return Ok(Language::all().filter(|l| supported(*l)).collect());
    }
    names
        .iter()
        .map(|n| Language::named(n).with_context(|| format!("rule `{id}`: unknown language `{n}`")))
        .collect()
}
