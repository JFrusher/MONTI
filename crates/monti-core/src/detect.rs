//! Per-file detectors a Tree-sitter query cannot express.

use crate::Edit;
use crate::ast::{Language, descendants, is_comment, is_directive, last_row};
use crate::metrics::functions;
use std::collections::HashSet;
use tree_sitter::Node;

pub struct Hit<'t> {
    pub node: Node<'t>,
    pub fix: Option<Edit>,
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
        .filter(|f| !is_method(*f) && !is_callback(*f) && is_pass_through(lang, *f, src))
        .map(|node| Hit { node, fix: None })
        .collect()
}

/// Methods may delegate to satisfy an interface or act as a facade.
fn is_method(f: Node) -> bool {
    if matches!(f.kind(), "method_definition" | "method_declaration") {
        return true;
    }
    let mut parent = f.parent();
    if parent.is_some_and(|p| p.kind() == "decorated_definition") {
        parent = parent.and_then(|p| p.parent());
    }
    parent
        .filter(|p| matches!(p.kind(), "block" | "declaration_list"))
        .and_then(|p| p.parent())
        .is_some_and(|c| matches!(c.kind(), "class_definition" | "impl_item" | "trait_item"))
}

/// A callback also pins the arity: `Array.from(xs, b => f(b))` is not `Array.from(xs, f)`,
/// because `Array.from` passes the index too.
fn is_callback(f: Node) -> bool {
    f.parent().is_some_and(|p| matches!(p.kind(), "arguments" | "argument_list"))
}

fn is_pass_through(lang: Language, func: Node, src: &str) -> bool {
    let Some(params) = param_names(lang, func, src) else { return false };
    // A zero-argument forwarder is usually an entry point or thunk, not a wrapper.
    if params.is_empty() {
        return false;
    }
    let Some(call) = func.child_by_field_name("body").and_then(single_call) else { return false };
    let Some(args) = call.child_by_field_name("arguments") else { return false };
    named(args).into_iter().map(|a| text(a, src)).eq(params)
}

fn single_call(mut n: Node) -> Option<Node> {
    const WRAPPERS: &[&str] = &[
        "statement_block",
        "block",
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
    matches!(n.kind(), "call_expression" | "call").then_some(n)
}

/// Parameter names in order, receivers (`self`, `cls`, Rust `&self`) dropped. `None` for shapes we do not model.
fn param_names<'s>(lang: Language, func: Node, src: &'s str) -> Option<Vec<&'s str>> {
    let list = func.child_by_field_name("parameters").or_else(|| func.child_by_field_name("parameter"))?;
    if list.kind() == "identifier" {
        return Some(vec![text(list, src)]);
    }
    let mut names = Vec::new();
    for p in named(list) {
        match p.kind() {
            "self_parameter" => {}
            "identifier" | "list_splat_pattern" | "dictionary_splat_pattern" => names.push(text(p, src)),
            "parameter_declaration" => {
                let mut cursor = p.walk();
                names.extend(p.children_by_field_name("name", &mut cursor).map(|n| text(n, src)));
            }
            "typed_parameter" => names.push(text(*named(p).first()?, src)),
            _ => {
                let name = p.child_by_field_name("pattern").or_else(|| p.child_by_field_name("name"))?;
                names.push(text(name, src));
            }
        }
    }
    if lang == Language::Python && matches!(names.first(), Some(&("self" | "cls"))) {
        names.remove(0);
    }
    Some(names)
}

// ---- redundant null checks -------------------------------------------------

/// `if (x != null)` right after `x` was assigned a value that cannot be null.
/// Fix (only when there is no `else`): replace the `if` with its dedented body.
pub fn redundant_null_checks<'t>(lang: Language, root: Node<'t>, src: &str) -> Vec<Hit<'t>> {
    let mut hits = Vec::new();
    for node in descendants(root) {
        if node.kind() != "if_statement" {
            continue;
        }
        let Some((name, value)) = node.prev_named_sibling().and_then(|p| assignment(p, src)) else { continue };
        let Some(cond) = node.child_by_field_name("condition") else { continue };
        if !non_null(lang, value, src) || !is_null_test(lang, text(cond, src), name) {
            continue;
        }
        let fix = match node.child_by_field_name("alternative") {
            None => unwrap_if(node, src),
            Some(_) => None,
        };
        hits.push(Hit { node, fix });
    }
    hits
}

/// `(name, value)` for a single-variable assignment or declaration statement.
fn assignment<'t, 's>(stmt: Node<'t>, src: &'s str) -> Option<(&'s str, Node<'t>)> {
    let mut n = stmt;
    if n.kind() == "expression_statement" {
        n = *named(n).first()?;
    }
    if matches!(n.kind(), "lexical_declaration" | "variable_declaration") {
        let &[decl] = named(n).as_slice() else { return None };
        n = decl;
    }
    let name = single(n.child_by_field_name("name").or_else(|| n.child_by_field_name("left"))?)?;
    let value = single(n.child_by_field_name("value").or_else(|| n.child_by_field_name("right"))?)?;
    (name.kind() == "identifier").then(|| (text(name, src), value))
}

/// Unwraps Go's one-element `expression_list`.
fn single(n: Node) -> Option<Node> {
    if n.kind() != "expression_list" {
        return Some(n);
    }
    let &[only] = named(n).as_slice() else { return None };
    Some(only)
}

fn non_null(lang: Language, v: Node, src: &str) -> bool {
    match lang {
        Language::TypeScript | Language::Tsx | Language::JavaScript => matches!(
            v.kind(),
            "new_expression" | "object" | "array" | "string" | "template_string" | "number" | "true" | "false"
                | "arrow_function" | "function_expression"
        ),
        Language::Python => matches!(
            v.kind(),
            "list" | "dictionary" | "set" | "tuple" | "string" | "integer" | "float" | "true" | "false"
                | "list_comprehension" | "dictionary_comprehension" | "set_comprehension" | "lambda"
        ),
        Language::Go => match v.kind() {
            "composite_literal" | "func_literal" | "interpreted_string_literal" | "raw_string_literal"
            | "int_literal" | "float_literal" => true,
            "unary_expression" => v.child_by_field_name("operand").is_some_and(|o| o.kind() == "composite_literal"),
            "call_expression" => v.child_by_field_name("function").is_some_and(|f| matches!(text(f, src), "new" | "make")),
            _ => false,
        },
        Language::Rust => false,
    }
}

fn is_null_test(lang: Language, cond: &str, name: &str) -> bool {
    let c: String = cond.chars().filter(|ch| !ch.is_whitespace() && !matches!(ch, '(' | ')')).collect();
    let (nulls, ops): (&[&str], &[&str]) = match lang {
        Language::TypeScript | Language::Tsx | Language::JavaScript => (&["null", "undefined"], &["!=", "!=="]),
        Language::Python => (&["None"], &["isnot", "!="]),
        Language::Go => (&["nil"], &["!="]),
        Language::Rust => (&[], &[]),
    };
    nulls.iter().any(|z| ops.iter().any(|op| c == format!("{name}{op}{z}") || c == format!("{z}{op}{name}")))
}

fn unwrap_if(if_node: Node, src: &str) -> Option<Edit> {
    let body = if_node.child_by_field_name("consequence")?;
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
        hits.push(Hit { node: first, fix: Some(delete_lines(first, last, src)) });
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
        .filter(|n| n.child_count() == 0 && n.kind().ends_with("identifier"))
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
