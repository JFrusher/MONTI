use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tree_sitter::{Node, Parser, Tree};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Rust,
    Go,
}

impl Language {
    pub fn from_path(path: &Path) -> Option<Language> {
        match path.extension()?.to_str()? {
            "ts" | "mts" | "cts" => Some(Language::TypeScript),
            "tsx" => Some(Language::Tsx),
            "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
            "py" => Some(Language::Python),
            "rs" => Some(Language::Rust),
            "go" => Some(Language::Go),
            _ => None,
        }
    }

    pub fn grammar(self) -> tree_sitter::Language {
        match self {
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            Language::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }

    /// Node kinds that count as a function for per-function checks.
    pub fn function_kinds(self) -> &'static [&'static str] {
        match self {
            Language::TypeScript | Language::Tsx | Language::JavaScript => &[
                "function_declaration",
                "function_expression",
                "arrow_function",
                "method_definition",
                "generator_function_declaration",
            ],
            Language::Python => &["function_definition"],
            Language::Rust => &["function_item"],
            Language::Go => &["function_declaration", "method_declaration", "func_literal"],
        }
    }
}

// shortcut: fixed list of tool directives; extend when one is treated as prose.
const DIRECTIVES: &[&str] = &[
    "#!", "go:", "+build", "eslint", "@ts-", "prettier-", "noqa", "type:", "pylint", "pragma", "nolint", "clippy",
    "istanbul",
];

/// Comments addressed to tools (`# type: ignore`, `// eslint-disable`), not to readers.
pub fn is_directive(comment: &str) -> bool {
    DIRECTIVES.iter().any(|d| comment.contains(d))
}

pub fn is_comment(kind: &str) -> bool {
    matches!(kind, "comment" | "line_comment" | "block_comment")
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
    if tree.root_node().has_error() {
        bail!("{lang:?} source has syntax errors");
    }
    Ok(tree)
}
