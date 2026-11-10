use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::Path;
use tree_sitter::{Parser, Tree};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    TypeScript,
    Python,
}

impl Language {
    pub fn from_path(path: &Path) -> Option<Language> {
        match path.extension()?.to_str()? {
            "ts" | "mts" | "cts" => Some(Language::TypeScript),
            "py" => Some(Language::Python),
            _ => None,
        }
    }

    pub fn grammar(self) -> tree_sitter::Language {
        match self {
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
        }
    }

    /// Node kinds that count as a function body for per-function metrics.
    pub fn function_kinds(self) -> &'static [&'static str] {
        match self {
            Language::TypeScript => &[
                "function_declaration",
                "function_expression",
                "arrow_function",
                "method_definition",
                "generator_function_declaration",
            ],
            Language::Python => &["function_definition"],
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
