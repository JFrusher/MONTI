use crate::languages::{SPECS, Spec};
use anyhow::{Context, Result, bail};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::path::Path;
use tree_sitter::{Node, Parser, Tree};

/// A supported language: a handle into [`crate::languages::SPECS`].
#[derive(Clone, Copy)]
pub struct Language(&'static Spec);

impl Language {
    pub fn named(name: &str) -> Option<Language> {
        SPECS.iter().find(|s| s.name == name).map(Language)
    }

    pub fn all() -> impl Iterator<Item = Language> {
        SPECS.iter().map(Language)
    }

    pub fn from_path(path: &Path) -> Option<Language> {
        let ext = path.extension()?.to_str()?;
        SPECS.iter().find(|s| s.extensions.contains(&ext)).map(Language)
    }

    pub fn name(self) -> &'static str {
        self.0.name
    }

    pub fn spec(self) -> &'static Spec {
        self.0
    }

    pub fn grammar(self) -> tree_sitter::Language {
        (self.0.grammar)()
    }
}

impl PartialEq for Language {
    fn eq(&self, other: &Language) -> bool {
        self.0.name == other.0.name
    }
}

impl Eq for Language {}

impl Hash for Language {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.name.hash(state);
    }
}

impl fmt::Debug for Language {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.0.name)
    }
}

impl Serialize for Language {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.name)
    }
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Language, D::Error> {
        let name = String::deserialize(deserializer)?;
        Language::named(&name).ok_or_else(|| {
            let known: Vec<&str> = Language::all().map(Language::name).collect();
            D::Error::custom(format!("unknown language `{name}`; known: {}", known.join(", ")))
        })
    }
}

// shortcut: fixed list of tool directives; extend when one is treated as prose.
const DIRECTIVES: &[&str] = &[
    "#!", "go:", "+build", "eslint", "@ts-", "prettier-", "noqa", "type:", "pylint", "pragma", "nolint", "clippy",
    "istanbul", "monti-ignore",
];

/// Comments addressed to tools (`# type: ignore`, `// eslint-disable`), not to readers.
pub fn is_directive(comment: &str) -> bool {
    DIRECTIVES.iter().any(|d| comment.contains(d))
}

/// Lowercase words, splitting on non-alphanumerics, camelCase and snake_case.
pub fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for ch in s.chars() {
        if (!ch.is_alphanumeric() || (ch.is_uppercase() && prev_lower)) && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if ch.is_alphanumeric() {
            cur.extend(ch.to_lowercase());
        }
        prev_lower = ch.is_lowercase() || ch.is_numeric();
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Commented-out code rather than prose: ends like a statement or block, or uses code operators.
pub fn looks_like_code(comment: &str) -> bool {
    let body = comment.trim().trim_end_matches("*/").trim_end();
    body.ends_with([';', '{', '}']) || ["::", "->", "<<", ">>", "&&", "||", "==", "!="].iter().any(|op| body.contains(op))
}

/// Comment node kinds across all grammars. A new grammar with another spelling adds it here.
pub fn is_comment(kind: &str) -> bool {
    matches!(kind, "comment" | "line_comment" | "block_comment" | "multiline_comment" | "documentation_comment")
}

/// Last row a node's text occupies. Some grammars end line comments after the newline.
pub fn last_row(n: Node) -> usize {
    let end = n.end_position();
    if end.column == 0 && end.row > n.start_position().row { end.row - 1 } else { end.row }
}

/// `root` and everything below it, in pre-order.
pub fn descendants(root: Node) -> Vec<Node> {
    let mut out = Vec::new();
    let mut cursor = root.walk();
    loop {
        out.push(cursor.node());
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return out;
            }
        }
    }
}

/// Parses `source`. Fails on syntax errors: rules matched against an error-recovered tree produce noise.
pub fn parse(lang: Language, source: &str) -> Result<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&lang.grammar())?;
    let tree = parser.parse(source, None).context("tree-sitter returned no tree")?;
    if let Some(bad) = descendants(tree.root_node()).into_iter().find(|n| n.is_error() || n.is_missing()) {
        let p = bad.start_position();
        bail!("{lang:?} syntax error at {}:{}", p.row + 1, p.column + 1);
    }
    Ok(tree)
}
