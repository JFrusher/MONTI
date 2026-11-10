use crate::ast::{Language, descendants, is_comment, is_directive, last_row, looks_like_code, words};
use std::collections::BTreeSet;
use tree_sitter::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metrics {
    /// Lines holding at least one non-comment token.
    pub code_lines: usize,
    /// Lines holding only comments. Trailing comments annotate code, so they do not count.
    pub comment_lines: usize,
    /// Comments, tool directives (`# type: ignore`) excluded.
    pub comment_count: usize,
    /// Depth of the deepest node below `node` (a leaf has depth 0).
    pub max_depth: usize,
}

pub fn measure(node: Node, src: &str) -> Metrics {
    let mut code = BTreeSet::new();
    let mut comments = BTreeSet::new();
    let mut comment_count = 0;
    let mut max_depth = 0;

    let mut stack = vec![(node, 0)];
    while let Some((n, depth)) = stack.pop() {
        max_depth = max_depth.max(depth);
        let rows = n.start_position().row..=n.end_position().row;
        if is_comment(n.kind()) {
            if !is_directive(&src[n.byte_range()]) {
                comment_count += 1;
                comments.extend(n.start_position().row..=last_row(n));
            }
        } else if n.child_count() == 0 {
            code.extend(rows);
        } else {
            let mut cursor = n.walk();
            stack.extend(n.children(&mut cursor).map(|c| (c, depth + 1)));
        }
    }

    Metrics {
        code_lines: code.len(),
        comment_lines: comments.difference(&code).count(),
        comment_count,
        max_depth,
    }
}

/// Lines of short own-line comments (at most 6 words: narration like `# Create figure`) per code
/// line. Full-sentence comments explain why and do not count, nor does commented-out code.
/// Consecutive comment lines are one comment: Python stores a two-line sentence as two nodes.
pub fn comment_code_ratio(node: Node, src: &str) -> f64 {
    let m = measure(node, src);
    let comments: Vec<Node> = descendants(node).into_iter().filter(|n| is_comment(n.kind())).collect();
    let mut blocks: Vec<Vec<Node>> = Vec::new();
    for c in comments {
        match blocks.last_mut() {
            Some(block) if last_row(*block.last().unwrap()) + 1 == c.start_position().row => block.push(c),
            _ => blocks.push(vec![c]),
        }
    }
    let narration: BTreeSet<usize> = blocks
        .iter()
        .filter(|block| {
            let text: String = block.iter().map(|c| &src[c.byte_range()]).collect::<Vec<_>>().join("\n");
            !is_directive(&text) && !looks_like_code(&text) && words(&text).len() <= 6
        })
        .flat_map(|block| block[0].start_position().row..=last_row(*block.last().unwrap()))
        .collect();
    let code: BTreeSet<usize> = descendants(node)
        .into_iter()
        .filter(|n| n.child_count() == 0 && !is_comment(n.kind()))
        .flat_map(|n| n.start_position().row..=n.end_position().row)
        .collect();
    narration.difference(&code).count() as f64 / m.code_lines as f64
}

/// Every function node under `root`, nested ones included.
pub fn functions(lang: Language, root: Node) -> Vec<Node> {
    let kinds = lang.spec().functions;
    descendants(root).into_iter().filter(|n| kinds.contains(&n.kind())).collect()
}
