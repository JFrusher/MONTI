//! Repo-wide symbol index: structural duplicates and ghost abstractions.

use crate::ast::{self, Language, descendants, is_comment};
use crate::metrics::functions;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use tree_sitter::Node;

/// Bodies smaller than this many AST nodes all look alike; they are not compared.
const MIN_NODES: usize = 20;

pub struct FnEntry {
    pub path: PathBuf,
    pub name: String,
    pub line: usize,
    pub column: usize,
    pub language: Language,
    range: Range<usize>,
    size: usize,
    shingles: HashSet<u64>,
}

pub struct Abstraction {
    pub path: PathBuf,
    pub name: String,
    pub line: usize,
    pub column: usize,
    pub language: Language,
}

pub struct Similar<'a> {
    pub proposed: String,
    pub existing: &'a FnEntry,
    pub similarity: f64,
}

#[derive(Default)]
pub struct Index {
    pub functions: Vec<FnEntry>,
    pub abstractions: Vec<Abstraction>,
    implementors: HashMap<String, usize>,
    occurrences: HashMap<String, usize>,
}

impl Index {
    pub fn build(root: &Path) -> Result<Index> {
        let mut index = Index::default();
        for (path, lang) in crate::source_files(root)? {
            let src = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            let tree = ast::parse(lang, &src).with_context(|| path.display().to_string())?;
            index.add(&path, lang, tree.root_node(), &src);
        }
        Ok(index)
    }

    pub fn add(&mut self, path: &Path, lang: Language, root: Node, src: &str) {
        for f in functions(lang, root) {
            let (shingles, size) = shingles(f, src);
            let p = f.start_position();
            self.functions.push(FnEntry {
                path: path.to_path_buf(),
                name: fn_name(f, src),
                line: p.row + 1,
                column: p.column + 1,
                language: lang,
                range: f.byte_range(),
                size,
                shingles,
            });
        }
        for n in descendants(root) {
            if n.child_count() == 0 && matches!(n.kind(), "identifier" | "type_identifier") {
                *self.occurrences.entry(src[n.byte_range()].to_string()).or_default() += 1;
            }
            if let Some(name) = abstraction_name(lang, n, src) {
                let p = n.start_position();
                self.abstractions.push(Abstraction {
                    path: path.to_path_buf(),
                    name: name.to_string(),
                    line: p.row + 1,
                    column: p.column + 1,
                    language: lang,
                });
            }
            for name in implemented(lang, n, src) {
                *self.implementors.entry(name.to_string()).or_default() += 1;
            }
        }
    }

    /// Pairs of distinct, non-nested functions in the same language at or above `min` similarity.
    // shortcut: pairs are pruned by size only; switch to MinHash/LSH if repos with huge function counts get slow.
    pub fn duplicates(&self, min: f64) -> Vec<(&FnEntry, &FnEntry, f64)> {
        let mut big: Vec<&FnEntry> = self.functions.iter().filter(|f| f.size >= MIN_NODES).collect();
        big.sort_by_key(|f| f.shingles.len());
        let mut out = Vec::new();
        for (i, a) in big.iter().enumerate() {
            for b in &big[i + 1..] {
                // Jaccard <= |a| / |b|, and |b| only grows from here.
                if (a.shingles.len() as f64) < min * b.shingles.len() as f64 {
                    break;
                }
                let nested = a.path == b.path && a.range.start < b.range.end && b.range.start < a.range.end;
                if a.language != b.language || nested {
                    continue;
                }
                let s = jaccard(&a.shingles, &b.shingles);
                if s >= min {
                    out.push((*a, *b, s));
                }
            }
        }
        out
    }

    /// Indexed functions that match a proposed function by name or by structure.
    pub fn similar(&self, lang: Language, root: Node, src: &str, min: f64) -> Vec<Similar<'_>> {
        let mut out = Vec::new();
        for f in functions(lang, root) {
            let proposed = fn_name(f, src);
            let (sh, size) = shingles(f, src);
            for e in self.functions.iter().filter(|e| e.language == lang) {
                let similarity = jaccard(&sh, &e.shingles);
                let structural = size >= MIN_NODES && e.size >= MIN_NODES && similarity >= min;
                if structural || e.name == proposed {
                    out.push(Similar { proposed: proposed.clone(), existing: e, similarity });
                }
            }
        }
        out
    }

    /// Abstractions with exactly one implementor and at most one use besides it and the declaration.
    pub fn ghosts(&self) -> Vec<&Abstraction> {
        self.abstractions
            .iter()
            .filter(|a| self.implementors.get(&a.name) == Some(&1) && self.occurrences.get(&a.name).is_some_and(|&n| n <= 3))
            .collect()
    }
}

fn fn_name(f: Node, src: &str) -> String {
    f.child_by_field_name("name").map_or("<anonymous>".into(), |n| src[n.byte_range()].to_string())
}

/// 3-grams of the function body's pre-order token sequence, plus the token count.
/// Only the body: matching signatures (interface stubs) are not duplicated logic.
fn shingles(f: Node, src: &str) -> (HashSet<u64>, usize) {
    let body = f.child_by_field_name("body").unwrap_or(f);
    let tokens: Vec<u64> =
        descendants(body).into_iter().filter(|n| !is_comment(n.kind())).map(|n| token(n, src)).collect();
    let set = tokens.windows(3).map(hash).collect();
    (set, tokens.len())
}

/// Identifiers reduce to their kind, so renamed copies match. Literal values stay,
/// so code differing only in its data (table-like tests) does not.
fn token(n: Node, src: &str) -> u64 {
    if n.is_named() && n.child_count() == 0 && !n.kind().ends_with("identifier") {
        hash((n.kind_id(), &src[n.byte_range()]))
    } else {
        n.kind_id() as u64
    }
}

fn hash(value: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

fn jaccard(a: &HashSet<u64>, b: &HashSet<u64>) -> f64 {
    let union = a.union(b).count();
    if union == 0 { 0.0 } else { a.intersection(b).count() as f64 / union as f64 }
}

fn abstraction_name<'s>(lang: Language, n: Node, src: &'s str) -> Option<&'s str> {
    let is_abstract = match lang {
        Language::TypeScript | Language::Tsx => matches!(n.kind(), "interface_declaration" | "abstract_class_declaration"),
        Language::Rust => n.kind() == "trait_item",
        Language::Python => {
            n.kind() == "class_definition"
                && n.child_by_field_name("superclasses").is_some_and(|s| {
                    s.utf8_text(src.as_bytes())
                        .unwrap_or_default()
                        .split(|c: char| !c.is_alphanumeric())
                        .any(|t| matches!(t, "ABC" | "ABCMeta"))
                })
        }
        Language::JavaScript | Language::Go => false,
    };
    if !is_abstract {
        return None;
    }
    n.child_by_field_name("name").map(|name| &src[name.byte_range()])
}

/// Names of the abstractions node `n` implements or extends.
fn implemented<'s>(lang: Language, n: Node, src: &'s str) -> HashSet<&'s str> {
    let heritage = match (lang, n.kind()) {
        (Language::TypeScript | Language::Tsx, "class_declaration" | "abstract_class_declaration") => {
            let mut cursor = n.walk();
            n.children(&mut cursor).find(|c| c.kind() == "class_heritage")
        }
        (Language::Python, "class_definition") => n.child_by_field_name("superclasses"),
        (Language::Rust, "impl_item") => n.child_by_field_name("trait"),
        _ => None,
    };
    let Some(heritage) = heritage else { return HashSet::new() };
    descendants(heritage)
        .into_iter()
        .filter(|c| c.child_count() == 0 && matches!(c.kind(), "identifier" | "type_identifier"))
        .map(|c| &src[c.byte_range()])
        .collect()
}
