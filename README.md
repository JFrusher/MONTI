# MONTI

Deterministic anti-slop code sweeper. Named for MSG Jared C. Monti.

Status: Phase 1 — `monti-core` only (TypeScript + Python).

## monti-core

- `ast::parse(lang, src)` — Tree-sitter parse; fails on syntax errors.
- `metrics::measure(node)` — code lines, comment lines, comment count, max AST depth.
- `rules::parse_rules(toml)` — loads `[rules.<id>]` tables; every query is compiled at load.
- `scan(lang, src, &rules)` — returns findings sorted by position.

## Rule schema

Each rule sets exactly one check: `tree_sitter_query` (finding points at the first capture)
or `max_comment_code_ratio` (per function). The table key is the rule id.

```toml
[rules.swallowed-catch]
severity = "error"            # "error" | "warning"
message = "Empty catch block."
languages = ["typescript"]    # "typescript" | "python"
tree_sitter_query = '''
(catch_clause body: (statement_block) @block (#match? @block "^\\{\\s*\\}$"))
'''
```

Defaults live in `rules/default.toml`. Run tests with `cargo test`.
