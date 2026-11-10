use monti_core::ast::{self, Language};
use monti_core::metrics;
use monti_core::rules::{DEFAULT_RULES, parse_rules};
use monti_core::scan;

fn hits(lang: Language, src: &str) -> Vec<(String, usize)> {
    let rules = parse_rules(DEFAULT_RULES).unwrap();
    scan(lang, src, &rules).unwrap().into_iter().map(|f| (f.rule_id, f.line)).collect()
}

fn owned(v: &[(&str, usize)]) -> Vec<(String, usize)> {
    v.iter().map(|(id, l)| (id.to_string(), *l)).collect()
}

#[test]
fn typescript_slop_is_flagged() {
    assert_eq!(
        hits(Language::TypeScript, include_str!("fixtures/slop.ts")),
        owned(&[("swallowed-catch", 4), ("swallowed-catch", 11), ("comment-flood", 15)])
    );
}

#[test]
fn typescript_clean_passes() {
    assert_eq!(hits(Language::TypeScript, include_str!("fixtures/clean.ts")), vec![]);
}

#[test]
fn python_slop_is_flagged() {
    assert_eq!(
        hits(Language::Python, include_str!("fixtures/slop.py")),
        owned(&[("swallowed-except", 5), ("comment-flood", 8)])
    );
}

#[test]
fn python_clean_passes() {
    assert_eq!(hits(Language::Python, include_str!("fixtures/clean.py")), vec![]);
}

#[test]
fn file_metrics() {
    let src = include_str!("fixtures/slop.py");
    let tree = ast::parse(Language::Python, src).unwrap();
    let m = metrics::measure(tree.root_node());
    assert_eq!((m.code_lines, m.comment_lines, m.comment_count), (7, 2, 2));
    assert!(m.max_depth >= 4, "depth {}", m.max_depth);
}

#[test]
fn syntax_errors_fail() {
    assert!(ast::parse(Language::Python, "def (:\n").is_err());
}

#[test]
fn bad_rules_fail_at_load() {
    let bad_query = r#"
[rules.x]
severity = "error"
message = "m"
languages = ["python"]
tree_sitter_query = "(no_such_node) @n"
"#;
    let both_checks = r#"
[rules.x]
severity = "error"
message = "m"
languages = ["python"]
tree_sitter_query = "(pass_statement) @n"
max_comment_code_ratio = 0.5
"#;
    let unknown_language = r#"
[rules.x]
severity = "error"
message = "m"
languages = ["cobol"]
max_comment_code_ratio = 0.5
"#;
    for src in [bad_query, both_checks, unknown_language] {
        assert!(parse_rules(src).is_err(), "accepted: {src}");
    }
}
