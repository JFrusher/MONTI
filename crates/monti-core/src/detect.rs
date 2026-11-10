//! Per-file detectors a Tree-sitter query cannot express.

use crate::Edit;
use crate::ast::{Language, descendants, is_comment, is_directive, last_row};
use crate::languages::NullCheck;
use crate::metrics::functions;
use std::collections::HashSet;
use tree_sitter::Node;

pub struct Hit<'t> {
    pub node: Node<'t>,
    pub fix: Option<Edit>,
    /// Appended to the rule message, e.g. which name.
    pub note: Option<String>,
}

fn text<'s>(n: Node, src: &'s str) -> &'s str {
    &src[n.byte_range()]
}

/// Named children, comments excluded.
fn named(n: Node) -> Vec<Node> {
    let mut cursor = n.walk();
    n.named_children(&mut cursor).filter(|c| !is_comment(c.kind())).collect()
}

// ---- pass-through wrappers -------------------------------------------------

/// Free functions whose whole body is one call forwarding exactly their parameters, in order.
pub fn pass_through_wrappers<'t>(lang: Language, root: Node<'t>, src: &str) -> Vec<Hit<'t>> {
    functions(lang, root)
        .into_iter()
        .filter(|f| !is_method(lang, *f) && !is_callback(*f) && is_pass_through(lang, *f, src))
        .map(|node| Hit { node, fix: None, note: None })
        .collect()
}

/// Methods may delegate to satisfy an interface or act as a facade.
fn is_method(lang: Language, f: Node) -> bool {
    let spec = lang.spec();
    if spec.methods.contains(&f.kind()) {
        return true;
    }
    let mut parent = f.parent();
    if parent.is_some_and(|p| p.kind() == "decorated_definition") {
        parent = parent.and_then(|p| p.parent());
    }
    let grandparent = parent.and_then(|p| p.parent());
    [parent, grandparent].into_iter().flatten().any(|n| spec.classes.contains(&n.kind()))
}

/// A callback also pins the arity: `Array.from(xs, b => f(b))` is not `Array.from(xs, f)`,
/// because `Array.from` passes the index too.
fn is_callback(f: Node) -> bool {
    f.parent().is_some_and(|p| matches!(p.kind(), "arguments" | "argument_list" | "argument"))
}

fn is_pass_through(lang: Language, func: Node, src: &str) -> bool {
    let Some(params) = param_names(func, src) else { return false };
    // A zero-argument forwarder is usually an entry point or thunk, not a wrapper.
    if params.is_empty() {
        return false;
    }
    let Some(call) = func.child_by_field_name("body").and_then(|b| single_call(lang, b)) else { return false };
    let Some(args) = call.child_by_field_name("arguments") else { return false };
    named(args).into_iter().map(|a| text(a, src)).eq(params)
}

fn single_call(lang: Language, mut n: Node) -> Option<Node> {
    const WRAPPERS: &[&str] = &[
        "statement_block",
        "compound_statement",
        "block",
        "body_statement",
        "function_body",
        "statement_list",
        "return_statement",
        "return_expression",
        "expression_statement",
        "expression_list",
    ];
    while WRAPPERS.contains(&n.kind()) {
        let &[only] = named(n).as_slice() else { return None };
        n = only;
    }
    lang.spec().calls.contains(&n.kind()).then_some(n)
}

/// The parameter list: on the function itself, or (C, C++, Dart) on its declarator or signature.
fn parameter_list(func: Node) -> Option<Node> {
    if let Some(p) = func.child_by_field_name("parameters").or_else(|| func.child_by_field_name("parameter")) {
        return Some(p);
    }
    let mut n = func.child_by_field_name("declarator").or_else(|| func.child_by_field_name("signature"))?;
    for _ in 0..3 {
        if let Some(p) = n.child_by_field_name("parameters") {
            return Some(p);
        }
        n = *named(n).first()?;
    }
    None
}

/// Parameter names in order, receivers (`self`, `cls`, Rust `&self`) dropped. `None` for shapes we do not model.
fn param_names<'s>(func: Node, src: &'s str) -> Option<Vec<&'s str>> {
    let list = parameter_list(func)?;
    if list.kind() == "identifier" {
        return Some(vec![text(list, src)]);
    }
    let mut names = Vec::new();
    for p in named(list) {
        match p.kind() {
            "self_parameter" => {}
            "identifier" | "list_splat_pattern" | "dictionary_splat_pattern" => names.push(text(p, src)),
            "typed_parameter" => names.push(text(*named(p).first()?, src)),
            _ => {
                let mut cursor = p.walk();
                let multi: Vec<Node> = p.children_by_field_name("name", &mut cursor).collect();
                if multi.len() > 1 {
                    // Go: `a, b int`
                    names.extend(multi.iter().map(|n| text(*n, src)));
                    continue;
                }
                let name = p
                    .child_by_field_name("pattern")
                    .or_else(|| p.child_by_field_name("name"))
                    .or_else(|| p.child_by_field_name("declarator").filter(|d| d.kind() == "identifier"))?;
                names.push(text(name, src));
            }
        }
    }
    if matches!(names.first(), Some(&("self" | "cls"))) {
        names.remove(0);
    }
    Some(names)
}

// ---- redundant null checks -------------------------------------------------

/// `if (x != null)` right after `x` was assigned a value that cannot be null.
/// Fix (only for a plain `if` without `else`/`elif`/initializer): replace it with its dedented body.
pub fn redundant_null_checks<'t>(lang: Language, root: Node<'t>, src: &str) -> Vec<Hit<'t>> {
    let Some(check) = &lang.spec().null_check else { return Vec::new() };
    let mut hits = Vec::new();
    for node in descendants(root) {
        if node.kind() != "if_statement" {
            continue;
        }
        let Some((name, value)) = node.prev_named_sibling().and_then(|p| assignment(p, src)) else { continue };
        let Some(cond) = node.child_by_field_name("condition") else { continue };
        if !(check.non_null)(value, src) || !is_null_test(check, text(cond, src), name) {
            continue;
        }
        let fix = if named(node).len() == 2 { unwrap_if(node, src) } else { None };
        hits.push(Hit { node, fix, note: None });
    }
    hits
}

/// `(name, value)` for a single-variable assignment or declaration statement.
fn assignment<'t, 's>(stmt: Node<'t>, src: &'s str) -> Option<(&'s str, Node<'t>)> {
    const DECLARATIONS: &[&str] = &["lexical_declaration", "variable_declaration", "local_variable_declaration", "declaration"];
    const DECLARATORS: &[&str] = &["variable_declarator", "init_declarator"];
    const NAMES: &[&str] = &["identifier", "variable_name", "name"];
    let mut n = stmt;
    if n.kind() == "expression_statement" {
        n = *named(n).first()?;
    }
    if DECLARATIONS.contains(&n.kind()) {
        let decls: Vec<Node> = named(n).into_iter().filter(|c| DECLARATORS.contains(&c.kind())).collect();
        let &[decl] = decls.as_slice() else { return None };
        n = decl;
    }
    let name = n.child_by_field_name("name").or_else(|| n.child_by_field_name("left")).or_else(|| n.child_by_field_name("declarator"));
    let name = single(name?)?;
    let value = single(n.child_by_field_name("value").or_else(|| n.child_by_field_name("right"))?)?;
    NAMES.contains(&name.kind()).then(|| (text(name, src), value))
}

/// Unwraps Go's one-element `expression_list`.
fn single(n: Node) -> Option<Node> {
    if n.kind() != "expression_list" {
        return Some(n);
    }
    let &[only] = named(n).as_slice() else { return None };
    Some(only)
}

fn is_null_test(check: &NullCheck, cond: &str, name: &str) -> bool {
    let c: String = cond.chars().filter(|ch| !ch.is_whitespace() && !matches!(ch, '(' | ')')).collect();
    check
        .nulls
        .iter()
        .any(|z| check.ops.iter().any(|op| c == format!("{name}{op}{z}") || c == format!("{z}{op}{name}")))
}

fn unwrap_if(if_node: Node, src: &str) -> Option<Edit> {
    let body = if_node.child_by_field_name("consequence").or_else(|| if_node.child_by_field_name("body"))?;
    let mut cursor = body.walk();
    let mut stmts: Vec<Node> = body.named_children(&mut cursor).collect();
    if let &[list] = stmts.as_slice()
        && list.kind() == "statement_list"
    {
        let mut cursor = list.walk();
        stmts = list.named_children(&mut cursor).collect();
    }
    let (first, last) = (stmts.first()?, stmts.last()?);
    let delta = first.start_position().column.saturating_sub(if_node.start_position().column);
    let mut replacement = String::new();
    for (i, line) in src[first.start_byte()..last.end_byte()].split('\n').enumerate() {
        if i == 0 {
            replacement.push_str(line);
        } else {
            let indent = line.len() - line.trim_start().len();
            replacement.push('\n');
            replacement.push_str(&line[indent.min(delta)..]);
        }
    }
    Some(Edit { start: if_node.start_byte(), end: if_node.end_byte(), replacement })
}

// ---- undefined names ------------------------------------------------------

/// Bare calls to a name that appears nowhere else in the file and is not a builtin: in a dynamic
/// language that is a NameError waiting to happen, typically a hallucinated helper.
pub fn undefined_names<'t>(lang: Language, root: Node<'t>, src: &str) -> Vec<Hit<'t>> {
    let spec = lang.spec();
    let Some(undefined) = &spec.undefined else { return Vec::new() };
    let nodes = descendants(root);
    if nodes.iter().any(|n| n.kind() == undefined.wildcard_import) {
        return Vec::new();
    }
    let mut callees = Vec::new();
    let mut elsewhere = HashSet::new();
    for n in nodes.into_iter().filter(|n| n.kind() == "identifier") {
        let callee = n.parent().is_some_and(|p| spec.calls.contains(&p.kind()) && p.child_by_field_name("function") == Some(n));
        if callee {
            callees.push(n);
        } else {
            elsewhere.insert(text(n, src));
        }
    }
    callees
        .into_iter()
        .filter(|n| !elsewhere.contains(text(*n, src)) && !undefined.builtins.contains(&text(*n, src)))
        .map(|node| Hit { node, fix: None, note: Some(format!("`{}`", text(node, src))) })
        .collect()
}

// ---- unused parameters ------------------------------------------------------

/// Free functions with a parameter the body never mentions. Methods, callbacks and decorated
/// functions are skipped (their signature is usually imposed), as are stubs, `_`-prefixed names and
/// splats.
pub fn unused_parameters<'t>(lang: Language, root: Node<'t>, src: &str) -> Vec<Hit<'t>> {
    let mut hits = Vec::new();
    for f in functions(lang, root) {
        if is_method(lang, f) || is_callback(f) || f.parent().is_some_and(|p| p.kind() == "decorated_definition") {
            continue;
        }
        // Test runners inject parameters by name (pytest fixtures, Go's `t`): a fixture requested
        // only for its side effect is not unused.
        if f.child_by_field_name("name").is_some_and(|n| text(n, src).to_lowercase().starts_with("test")) {
            continue;
        }
        let (Some(params), Some(body)) = (param_names(f, src), f.child_by_field_name("body")) else { continue };
        if is_stub(body) {
            continue;
        }
        // Whole words of the body text, not just identifier nodes: names used inside format strings
        // (`format!("{name}")`) and interpolations are not identifiers in every grammar.
        let used: HashSet<&str> = text(body, src).split(|c: char| !(c.is_alphanumeric() || c == '_')).collect();
        // Plain names only (`x`, PHP `$x`, Rust `mut x`); destructuring patterns and splats are skipped.
        let unused: Vec<String> = params
            .iter()
            .map(|p| p.trim_start_matches('$').trim_start_matches("mut "))
            .filter(|p| p.chars().all(|c| c.is_alphanumeric() || c == '_'))
            .filter(|name| !name.is_empty() && !name.starts_with('_') && !used.contains(name))
            .map(|name| format!("`{name}`"))
            .collect();
        if !unused.is_empty() {
            hits.push(Hit { node: f, fix: None, note: Some(unused.join(", ")) });
        }
    }
    hits
}

/// A body that only declares the signature: `...`, `pass`, `raise NotImplementedError`, or nothing.
fn is_stub(body: Node) -> bool {
    match named(body).as_slice() {
        [] => true,
        [only] => {
            matches!(only.kind(), "pass_statement" | "ellipsis" | "raise_statement")
                || (only.kind() == "expression_statement" && named(*only).first().is_some_and(|e| e.kind() == "ellipsis"))
        }
        _ => false,
    }
}

// ---- restating comments ----------------------------------------------------

/// Notes and doc comments are never "restating", whatever their words.
const KEEP_MARKERS: &[&str] = &["TODO", "FIXME", "HACK", "SAFETY", "NOTE", "///", "//!", "/**"];

/// An own-line comment block whose words cover every identifier of the statement right below it,
/// e.g. `// Set age to 20` above `this.age = 20`. Fix: delete the comment lines.
pub fn restating_comments<'t>(root: Node<'t>, src: &str) -> Vec<Hit<'t>> {
    let mut hits = Vec::new();
    for last in descendants(root) {
        if !is_comment(last.kind()) || !own_line(last) {
            continue;
        }
        let Some(target) = last.next_named_sibling() else { continue };
        if is_comment(target.kind()) || target.start_position().row != last_row(last) + 1 {
            continue;
        }
        let mut first = last;
        while let Some(p) = first.prev_named_sibling()
            && is_comment(p.kind())
            && own_line(p)
            && last_row(p) + 1 == first.start_position().row
        {
            first = p;
        }
        let comment = &src[first.start_byte()..last.end_byte()];
        if is_directive(comment) || KEEP_MARKERS.iter().any(|m| comment.contains(m)) || !restates(comment, target, src) {
            continue;
        }
        hits.push(Hit { node: first, fix: Some(delete_lines(first, last, src)), note: None });
    }
    hits
}

fn own_line(c: Node) -> bool {
    c.prev_sibling().is_none_or(|p| last_row(p) < c.start_position().row)
}

fn restates(comment: &str, target: Node, src: &str) -> bool {
    let comment_words: HashSet<String> = words(comment).into_iter().collect();
    let mut ids: HashSet<String> = descendants(target)
        .into_iter()
        .filter(|n| n.child_count() == 0 && (n.kind().ends_with("identifier") || n.kind() == "name"))
        .flat_map(|n| words(text(n, src)))
        .collect();
    ids.remove("self");
    ids.remove("this");
    // The +4 allows filler like "set the ... to"; longer comments are explaining something.
    !ids.is_empty() && ids.is_subset(&comment_words) && comment_words.len() <= ids.len() + 4
}

/// Lowercase words, splitting on non-alphanumerics, camelCase and snake_case.
fn words(s: &str) -> Vec<String> {
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

/// Whole lines from `first`'s line through `last`'s line, newline included.
fn delete_lines(first: Node, last: Node, src: &str) -> Edit {
    let start = src[..first.start_byte()].rfind('\n').map_or(0, |i| i + 1);
    let end = if src[..last.end_byte()].ends_with('\n') {
        last.end_byte()
    } else {
        src[last.end_byte()..].find('\n').map_or(src.len(), |i| last.end_byte() + i + 1)
    };
    Edit { start, end, replacement: String::new() }
}

#[cfg(test)]
mod tests {
    #[test]
    fn words_split_case_and_punctuation() {
        assert_eq!(super::words("// setUserAge to_20"), ["set", "user", "age", "to", "20"]);
    }
}
