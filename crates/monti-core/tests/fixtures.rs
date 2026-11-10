use monti_core::ast::{self, Language};
use monti_core::rules::{DEFAULT_RULES, parse_rules};
use monti_core::{apply_fixes, metrics, scan, scan_path};
use std::fs;
use std::path::Path;

mod common;
use common::temp_dir;

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
        owned(&[
            ("swallowed-catch", 4),
            ("swallowed-catch", 11),
            ("comment-flood", 15),
            ("pass-through-wrapper", 21),
            ("redundant-null-check", 27),
            ("restating-comment", 36),
            ("commented-catch", 44),
        ])
    );
}

#[test]
fn python_slop_is_flagged() {
    assert_eq!(
        hits(Language::Python, include_str!("fixtures/slop.py")),
        owned(&[
            ("swallowed-except", 4),
            ("comment-flood", 8),
            ("pass-through-wrapper", 14),
            ("redundant-null-check", 20),
            ("comment-flood", 26),
            ("restating-comment", 27),
            ("commented-except", 34),
        ])
    );
}

#[test]
fn rust_slop_is_flagged() {
    assert_eq!(
        hits(Language::Rust, include_str!("fixtures/slop.rs")),
        owned(&[
            ("swallowed-err-arm", 4),
            ("pass-through-wrapper", 8),
            ("restating-comment", 13),
            ("commented-err-arm", 20),
        ])
    );
}

#[test]
fn go_slop_is_flagged() {
    assert_eq!(
        hits(Language::Go, include_str!("fixtures/slop.go")),
        owned(&[
            ("swallowed-err-check", 5),
            ("pass-through-wrapper", 9),
            ("redundant-null-check", 15),
            ("restating-comment", 22),
            ("commented-err-check", 29),
        ])
    );
}

#[test]
fn tsx_and_javascript_are_scanned() {
    assert_eq!(Language::from_path(Path::new("a.tsx")), Some(Language::Tsx));
    for ext in ["js", "jsx", "mjs", "cjs"] {
        assert_eq!(Language::from_path(Path::new(&format!("a.{ext}"))), Some(Language::JavaScript));
    }
    assert_eq!(hits(Language::Tsx, include_str!("fixtures/slop.tsx")), owned(&[("swallowed-catch", 4)]));
    assert_eq!(
        hits(Language::JavaScript, include_str!("fixtures/slop.js")),
        owned(&[("commented-catch", 4), ("pass-through-wrapper", 9)])
    );
}

#[test]
fn empty_catches_are_errors_commented_ones_warnings() {
    let rules = parse_rules(DEFAULT_RULES).unwrap();
    let ids = |lang, src: &str| -> Vec<String> { scan(lang, src, &rules).unwrap().into_iter().map(|f| f.rule_id).collect() };
    let py = |body: &str| format!("try:\n    go()\nexcept E:\n{body}\n");
    assert_eq!(ids(Language::Python, &py("    pass")), ["swallowed-except"]);
    assert_eq!(ids(Language::Python, &py("    # deliberate\n    pass")), ["commented-except"]);
    assert_eq!(ids(Language::Python, &py("    pass  # deliberate")), ["commented-except"]);
    assert_eq!(ids(Language::TypeScript, "try { go(); } catch { }"), ["swallowed-catch"]);
    assert_eq!(ids(Language::TypeScript, "try { go(); } catch { /* private window */ }"), ["commented-catch"]);
    assert_eq!(ids(Language::TypeScript, "try { go(); } catch { /* a */ log(); /* b */ }"), Vec::<String>::new());
}

#[test]
fn clean_fixtures_pass() {
    assert_eq!(hits(Language::TypeScript, include_str!("fixtures/clean.ts")), vec![]);
    assert_eq!(hits(Language::Python, include_str!("fixtures/clean.py")), vec![]);
    assert_eq!(hits(Language::Rust, include_str!("fixtures/clean.rs")), vec![]);
    assert_eq!(hits(Language::Go, include_str!("fixtures/clean.go")), vec![]);
}

#[test]
fn fixes_unwrap_null_checks_and_delete_restating_comments() {
    let src = include_str!("fixtures/slop.py");
    let rules = parse_rules(DEFAULT_RULES).unwrap();
    let findings = scan(Language::Python, src, &rules).unwrap();
    let edits: Vec<_> = findings.iter().filter_map(|f| f.fix.as_ref()).collect();
    assert_eq!(edits.len(), 2);
    let fixed = apply_fixes(src, &edits);
    assert!(fixed.contains("    items = []\n    items.append(1)\n    return items\n"), "{fixed}");
    assert!(fixed.contains("    def grow(self):\n        self.age += 1\n"), "{fixed}");
    ast::parse(Language::Python, &fixed).unwrap();

    let src = include_str!("fixtures/slop.go");
    let findings = scan(Language::Go, src, &rules).unwrap();
    let edits: Vec<_> = findings.iter().filter_map(|f| f.fix.as_ref()).collect();
    let fixed = apply_fixes(src, &edits);
    assert!(fixed.contains("\ts := &Store{}\n\ts.init()\n\treturn s\n"), "{fixed}");
    assert!(fixed.contains("func Bump(count int) int {\n\tcount++\n"), "{fixed}");
}

#[test]
fn repo_scan_finds_duplicates_and_ghosts() {
    let dir = temp_dir("repo");
    fs::write(
        dir.join("a.ts"),
        "export function sumPrices(items: Item[]): number {\n  let total = 0;\n  for (const item of items) {\n    if (item.price > 0) {\n      total += item.price * item.qty;\n    }\n  }\n  return total;\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("b.ts"),
        "export function weightOf(parcels: Parcel[]): number {\n  let acc = 0;\n  for (const p of parcels) {\n    if (p.mass > 0) {\n      acc += p.mass * p.count;\n    }\n  }\n  return acc;\n}\n\nexport function names(xs: { name: string }[]): string {\n  return xs.map((x) => x.name.trim()).join(', ');\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("ghost.ts"),
        "interface Repo { get(id: string): string }\nclass SqlRepo implements Repo { get(id: string) { return id.trim(); } }\nexport const repo: Repo = new SqlRepo();\n\ninterface Shape { area(): number }\nclass Sq implements Shape { area() { return 1; } }\nclass Ci implements Shape { area() { return 2; } }\n",
    )
    .unwrap();
    fs::write(dir.join("ghost.py"), "from abc import ABC\n\nclass Store(ABC):\n    pass\n\nclass DiskStore(Store):\n    pass\n").unwrap();
    fs::write(dir.join("ghost.rs"), "trait Clock { fn now(&self) -> u64; }\nstruct Sys;\nimpl Clock for Sys { fn now(&self) -> u64 { 0 } }\n").unwrap();
    // Same body shape, different data: not a copy. Same signature, stub body: not a copy.
    fs::write(
        dir.join("not_copies.py"),
        "def test_interpolates():\n    assert sim_time(5, [(0, 100.0), (10, 200.0)]) == 150.0\n\n\ndef test_holds():\n    assert sim_time(20, [(0, 100.0), (10, 200.0)]) == 200.0\n\n\nclass Api(Protocol):\n    def messages(self, endpoint: str) -> list[dict[str, Any]]: ...\n    def patients(self, endpoint: str) -> list[dict[str, Any]]: ...\n",
    )
    .unwrap();
    fs::write(dir.join(".montiignore"), "ignored/\n").unwrap();
    fs::create_dir(dir.join("ignored")).unwrap();
    fs::write(dir.join("ignored/x.py"), "def (:\n").unwrap();

    let reports = scan_path(&dir, &parse_rules(DEFAULT_RULES).unwrap()).unwrap();
    let got: Vec<(String, String, usize)> = reports
        .iter()
        .flat_map(|r| {
            let name = r.path.file_name().unwrap().to_string_lossy().to_string();
            r.findings.iter().map(move |f| (name.clone(), f.rule_id.clone(), f.line))
        })
        .collect();
    let want: Vec<(String, String, usize)> = [
        ("a.ts", "duplicate-function", 1),
        ("b.ts", "duplicate-function", 1),
        ("ghost.py", "ghost-abstraction", 3),
        ("ghost.rs", "ghost-abstraction", 1),
        ("ghost.ts", "ghost-abstraction", 1),
    ]
    .iter()
    .map(|(a, b, c)| (a.to_string(), b.to_string(), *c))
    .collect();
    assert_eq!(got, want);
    assert!(reports[0].findings[0].message.contains("100% match with `weightOf`"));
}

#[test]
fn file_metrics() {
    let src = "def f(x):\n    # why\n    return x  # trailing\n    # type: ignore\n";
    let tree = ast::parse(Language::Python, src).unwrap();
    let m = metrics::measure(tree.root_node(), src);
    // own-line "# why" only: trailing comments annotate code, directives are for tools
    assert_eq!((m.code_lines, m.comment_lines, m.comment_count), (2, 1, 2));
    assert!(m.max_depth >= 3, "depth {}", m.max_depth);
}

#[test]
fn syntax_errors_fail() {
    assert!(ast::parse(Language::Python, "def (:\n").is_err());
}

#[test]
fn bad_rules_fail_at_load() {
    let rule = |body: &str| format!("[rules.x]\nseverity = \"error\"\nmessage = \"m\"\n{body}");
    for body in [
        "languages = [\"python\"]\ntree_sitter_query = \"(no_such_node) @n\"",
        "languages = [\"python\"]\ntree_sitter_query = \"(pass_statement) @n\"\nmax_comment_code_ratio = 0.5",
        "languages = [\"cobol\"]\nmax_comment_code_ratio = 0.5",
        "languages = [\"rust\"]\ncheck = \"redundant-null-check\"",
        "languages = [\"go\"]\ncheck = \"duplicate-function\"",
        "languages = [\"go\"]\ncheck = \"restating-comment\"\nmin_similarity = 0.5",
        "languages = [\"go\"]\ncheck = \"no-such-check\"",
    ] {
        assert!(parse_rules(&rule(body)).is_err(), "accepted: {body}");
    }
}
