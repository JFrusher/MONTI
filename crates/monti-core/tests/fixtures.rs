use monti_core::ast::{self, Language};
use monti_core::rules::{DEFAULT_RULES, parse_rules};
use monti_core::{apply_fixes, metrics, scan, scan_path};
use std::fs;
use std::path::Path;

mod common;
use common::temp_dir;

fn py() -> Language {
    Language::named("python").unwrap()
}

fn go() -> Language {
    Language::named("go").unwrap()
}

fn ts() -> Language {
    Language::named("typescript").unwrap()
}


#[test]
fn empty_catches_are_errors_commented_ones_warnings() {
    let rules = parse_rules(DEFAULT_RULES).unwrap();
    let ids = |lang, src: &str| -> Vec<String> { scan(lang, src, &rules).unwrap().into_iter().map(|f| f.rule_id).collect() };
    let try_except = |body: &str| format!("try:\n    go()\nexcept E:\n{body}\n");
    assert_eq!(ids(py(), &try_except("    pass")), ["swallowed-error"]);
    assert_eq!(ids(py(), &try_except("    # deliberate\n    pass")), ["commented-error"]);
    assert_eq!(ids(py(), &try_except("    pass  # deliberate")), ["commented-error"]);
    assert_eq!(ids(ts(), "try { go(); } catch { }"), ["swallowed-error"]);
    assert_eq!(ids(ts(), "try { go(); } catch { /* private window */ }"), ["commented-error"]);
    assert_eq!(ids(ts(), "try { go(); } catch { /* a */ log(); /* b */ }"), Vec::<String>::new());
}

/// Every registered language must ship `fixtures/<name>/slop.<ext>` with a `slop.expect`
/// (one `LINE RULE` per finding) and a `clean.<ext>` with no findings.
#[test]
fn every_language_matches_its_fixtures() {
    let rules = parse_rules(DEFAULT_RULES).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut failures = Vec::new();
    for lang in Language::all() {
        let dir = root.join(lang.name());
        let ext = lang.spec().extensions[0];
        let read = |f: String| fs::read_to_string(dir.join(&f)).unwrap_or_else(|_| panic!("missing fixtures/{}/{f}", lang.name()));
        let found = |src: &str| -> Vec<String> {
            match scan(lang, src, &rules) {
                Ok(fs) => fs.iter().map(|f| format!("{} {}", f.line, f.rule_id)).collect(),
                Err(e) => vec![format!("ERROR {e:#}")],
            }
        };
        let want: Vec<String> = read("slop.expect".into()).lines().map(str::to_string).collect();
        let got = found(&read(format!("slop.{ext}")));
        if got != want {
            failures.push(format!("{}/slop.{ext}\n  want {want:?}\n  got  {got:?}", lang.name()));
        }
        let got = found(&read(format!("clean.{ext}")));
        if !got.is_empty() {
            failures.push(format!("{}/clean.{ext}\n  got  {got:?}", lang.name()));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn fixes_unwrap_null_checks_and_delete_restating_comments() {
    let src = include_str!("fixtures/python/slop.py");
    let rules = parse_rules(DEFAULT_RULES).unwrap();
    let findings = scan(py(), src, &rules).unwrap();
    let edits: Vec<_> = findings.iter().filter_map(|f| f.fix.as_ref()).collect();
    assert_eq!(edits.len(), 2);
    let fixed = apply_fixes(src, &edits);
    assert!(fixed.contains("    items = []\n    items.append(1)\n    return items\n"), "{fixed}");
    assert!(fixed.contains("    def grow(self):\n        self.age += 1\n"), "{fixed}");
    ast::parse(py(), &fixed).unwrap();

    let src = include_str!("fixtures/go/slop.go");
    let findings = scan(go(), src, &rules).unwrap();
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
    fs::write(dir.join("broken.py"), "ML unit tests.\n").unwrap();
    fs::write(dir.join("bundle.js"), format!("var a=1;{}\n", "f(a);".repeat(100))).unwrap();
    fs::write(dir.join(".montiignore"), "ignored/\n").unwrap();
    fs::create_dir(dir.join("ignored")).unwrap();
    fs::write(dir.join("ignored/x.py"), "def (:\n").unwrap();

    let scan = scan_path(&dir, &parse_rules(DEFAULT_RULES).unwrap()).unwrap();
    let skipped: Vec<(String, String)> =
        scan.skipped.iter().map(|s| (s.path.file_name().unwrap().to_string_lossy().into(), s.reason.clone())).collect();
    assert_eq!(
        skipped,
        [("broken.py".into(), "python syntax error at 1:1".into()), ("bundle.js".into(), "minified".into())]
    );
    let reports = scan.files;
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
    let tree = ast::parse(py(), src).unwrap();
    let m = metrics::measure(tree.root_node(), src);
    // own-line "# why" only: trailing comments annotate code, directives are for tools
    assert_eq!((m.code_lines, m.comment_lines, m.comment_count), (2, 1, 2));
    assert!(m.max_depth >= 3, "depth {}", m.max_depth);
}

#[test]
fn syntax_errors_fail() {
    assert!(ast::parse(py(), "def (:\n").is_err());
}

#[test]
fn bad_rules_fail_at_load() {
    let rule = |body: &str| format!("[rules.x]\nseverity = \"error\"\nmessage = \"m\"\n{body}");
    // Control: the shared header is valid, so each case below fails for its own reason.
    parse_rules(&rule("queries = { python = \"(pass_statement) @n\" }")).unwrap();
    parse_rules(&rule("languages = [\"*\"]\ncheck = \"restating-comment\"")).unwrap();
    for (body, reason) in [
        ("queries = { python = \"(no_such_node) @n\" }", "invalid query"),
        ("queries = { python = \"(pass_statement)\" }", "no capture"),
        ("queries = { cobol = \"(x) @n\" }", "unknown language"),
        ("queries = { python = \"(pass_statement) @n\" }\nlanguages = [\"python\"]", "drop `languages`"),
        ("queries = { python = \"(pass_statement) @n\" }\nmax_comment_code_ratio = 0.5", "exactly one"),
        ("languages = [\"cobol\"]\nmax_comment_code_ratio = 0.5", "unknown language"),
        ("max_comment_code_ratio = 0.5", "needs `languages`"),
        ("languages = [\"rust\"]\ncheck = \"redundant-null-check\"", "does not support"),
        ("languages = [\"go\"]\ncheck = \"duplicate-function\"", "needs `min_similarity`"),
        ("languages = [\"go\"]\ncheck = \"restating-comment\"\nmin_similarity = 0.5", "only applies"),
        ("languages = [\"go\"]\ncheck = \"no-such-check\"", "unknown variant"),
    ] {
        let err = format!("{:#}", parse_rules(&rule(body)).expect_err(body));
        assert!(err.contains(reason), "{body}\n  wanted `{reason}`, got: {err}");
    }
}
