use crate::ast::Language;
use std::collections::BTreeSet;
use tree_sitter::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metrics {
    /// Lines holding at least one non-comment token.
    pub code_lines: usize,
    /// Lines holding at least one comment. A line with code and a trailing comment counts in both.
    pub comment_lines: usize,
    pub comment_count: usize,
    /// Depth of the deepest node below `node` (a leaf has depth 0).
    pub max_depth: usize,
}

pub fn measure(node: Node) -> Metrics {
    let mut code = BTreeSet::new();
    let mut comments = BTreeSet::new();
    let mut comment_count = 0;
    let mut max_depth = 0;

    let mut stack = vec![(node, 0)];
    while let Some((n, depth)) = stack.pop() {
        max_depth = max_depth.max(depth);
        let rows = n.start_position().row..=n.end_position().row;
        if n.kind() == "comment" {
            comment_count += 1;
            comments.extend(rows);
        } else if n.child_count() == 0 {
            code.extend(rows);
        } else {
            let mut cursor = n.walk();
            stack.extend(n.children(&mut cursor).map(|c| (c, depth + 1)));
        }
    }

    Metrics {
        code_lines: code.len(),
        comment_lines: comments.len(),
        comment_count,
        max_depth,
    }
}

pub fn comment_code_ratio(node: Node) -> f64 {
    let m = measure(node);
    m.comment_lines as f64 / m.code_lines as f64
}

/// Every function node under `root`, nested ones included.
pub fn functions(lang: Language, root: Node) -> Vec<Node> {
    let kinds = lang.function_kinds();
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if kinds.contains(&n.kind()) {
            found.push(n);
        }
        let mut cursor = n.walk();
        stack.extend(n.children(&mut cursor));
    }
    found
}
