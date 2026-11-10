//! Repo-wide symbol index: structural duplicates and ghost abstractions.

use crate::ast::{Language, descendants, is_comment};
use crate::metrics::functions;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};
use tree_sitter::Node;

/// MinHash signature length, cut into BANDS bands of SIGNATURE / BANDS rows. Two functions are
/// compared exactly only when some band matches. At Jaccard 0.85 a pair is missed with
/// probability (1 - 0.85^4)^16 < 1e-5; at 0.5 it is still a candidate 64% of the time.
const SIGNATURE: usize = 64;
const BANDS: usize = 16;

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
    signature: [u64; SIGNATURE],
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
    /// Indexes every file under `root` that parses. Skipped files are left out.
    pub fn build(root: &Path) -> Result<Index> {
        let sources = crate::load_sources(root)?.0;
        let parts = crate::par_map(&sources, |s| Index::of_file(&s.path, s.language, s.tree.root_node(), &s.text));
        let mut index = Index::default();
        parts.into_iter().for_each(|p| index.merge(p));
        Ok(index)
    }

    /// Merges another index (one file's, usually) into this one.
    pub fn merge(&mut self, other: Index) {
        self.functions.extend(other.functions);
        self.abstractions.extend(other.abstractions);
        for (name, n) in other.implementors {
            *self.implementors.entry(name).or_default() += n;
        }
        for (name, n) in other.occurrences {
            *self.occurrences.entry(name).or_default() += n;
        }
    }

    /// The index of a single file.
    pub fn of_file(path: &Path, lang: Language, root: Node, src: &str) -> Index {
        let mut index = Index::default();
        index.add(path, lang, root, src);
        index
    }

    fn add(&mut self, path: &Path, lang: Language, root: Node, src: &str) {
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
                signature: minhash(&shingles),
                shingles,
            });
        }
        for n in descendants(root) {
            if n.child_count() == 0 && NAME_KINDS.contains(&n.kind()) {
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
    pub fn duplicates(&self, min: f64) -> Vec<(&FnEntry, &FnEntry, f64)> {
        let big: Vec<&FnEntry> = self.functions.iter().filter(|f| f.size >= MIN_NODES).collect();
        let rows = SIGNATURE / BANDS;
        let mut buckets: HashMap<(usize, &[u64]), Vec<usize>> = HashMap::new();
        for (i, f) in big.iter().enumerate() {
            for band in 0..BANDS {
                buckets.entry((band, &f.signature[band * rows..(band + 1) * rows])).or_default().push(i);
            }
        }
        let mut candidates: HashSet<(usize, usize)> = HashSet::new();
        for ids in buckets.values() {
            for (k, &i) in ids.iter().enumerate() {
                candidates.extend(ids[k + 1..].iter().map(|&j| (i, j)));
            }
        }
        let mut pairs: Vec<(usize, usize)> = candidates.into_iter().collect();
        pairs.sort_unstable();
        let mut out = Vec::new();
        for (i, j) in pairs {
            let (a, b) = (big[i], big[j]);
            let nested = a.path == b.path && a.range.start < b.range.end && b.range.start < a.range.end;
            if a.language != b.language || nested {
                continue;
            }
            let s = jaccard(&a.shingles, &b.shingles);
            if s >= min {
                out.push((a, b, s));
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

/// Leaf kinds that hold a type or symbol name, across grammars.
const NAME_KINDS: &[&str] = &["identifier", "type_identifier", "name", "constant"];

/// The `name` field, or (C, C++, Dart) the name at the end of the declarator/signature chain.
fn fn_name(f: Node, src: &str) -> String {
    let mut n = f;
    for _ in 0..4 {
        if let Some(name) = n.child_by_field_name("name") {
            return src[name.byte_range()].to_string();
        }
        match n.child_by_field_name("declarator").or_else(|| n.child_by_field_name("signature")) {
            Some(next) => n = next,
            None => break,
        }
    }
    if n != f && matches!(n.kind(), "identifier" | "field_identifier") {
        return src[n.byte_range()].to_string();
    }
    "<anonymous>".into()
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

/// One minimum per seeded hash function over the shingle set.
fn minhash(shingles: &HashSet<u64>) -> [u64; SIGNATURE] {
    let mut sig = [u64::MAX; SIGNATURE];
    for &s in shingles {
        for (seed, slot) in sig.iter_mut().enumerate() {
            *slot = (*slot).min(mix(s ^ (seed as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)));
        }
    }
    sig
}

/// splitmix64 finalizer: a cheap, well-spread 64-bit mix.
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

pub(crate) fn hash(value: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

fn jaccard(a: &HashSet<u64>, b: &HashSet<u64>) -> f64 {
    let union = a.union(b).count();
    if union == 0 { 0.0 } else { a.intersection(b).count() as f64 / union as f64 }
}

fn abstraction_name<'s>(lang: Language, n: Node, src: &'s str) -> Option<&'s str> {
    let ghost = lang.spec().ghost.as_ref()?;
    if !(ghost.is_abstraction)(n, src) {
        return None;
    }
    n.child_by_field_name("name").map(|name| &src[name.byte_range()])
}

/// Names of the abstractions node `n` implements or extends.
fn implemented<'s>(lang: Language, n: Node, src: &'s str) -> HashSet<&'s str> {
    let Some(ghost) = &lang.spec().ghost else { return HashSet::new() };
    (ghost.heritage)(n)
        .into_iter()
        .flat_map(descendants)
        .filter(|c| c.child_count() == 0 && NAME_KINDS.contains(&c.kind()))
        .map(|c| &src[c.byte_range()])
        .collect()
}
