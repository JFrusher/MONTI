# Contributing to MONTI

Bug reports with a code sample, false positives, and new languages are all welcome.

## Ground rules

- Detection is deterministic: Tree-sitter queries and plain Rust. No LLM calls.
- A rule earns its place with evidence: a slop fixture it catches and a clean fixture it leaves alone.
- `cargo clippy --all-targets -- -D warnings` and `cargo test` pass before a PR.

## Add a language

1. **Grammar.** `cd crates/monti-core && cargo add tree-sitter-<lang>`. It must build against the
   workspace's `tree-sitter` version; `cargo build` tells you.
2. **Spec.** Add one entry to `SPECS` in `crates/monti-core/src/languages.rs`. Every field is
   documented there. Start with `name`, `extensions`, `grammar`, `functions`; leave the rest empty
   (`&[]`, `None`). An empty field only switches the detector that needs it off.
3. **Look at real trees.** `cargo run -p monti-cli -- tree some_file.<ext>` prints node kinds and
   field names. Use them for `functions`, `calls`, `classes` and queries.
4. **Fixtures.** Create `crates/monti-core/tests/fixtures/<name>/` with:
   - `slop.<ext>`: code the rules should flag;
   - `slop.expect`: one `LINE RULE` per expected finding, in order, e.g. `3 swallowed-error`;
   - `clean.<ext>`: idiomatic code that must produce no findings.

   `every_language_matches_its_fixtures` fails until these exist and match.
5. **Error-handling rules (optional).** Add a query for your language under
   `[rules.swallowed-error.queries]` and `[rules.commented-error.queries]` in `rules/default.toml`.
6. Run `cargo test` and open a PR.

## Add or change a rule

- Query rules live in `rules/default.toml` (one query per language under `queries`).
- Built-in detectors live in `crates/monti-core/src/detect.rs` (per file) and `index.rs` (repo-wide).
- Add the case to a fixture and its `.expect`. For a false positive, add the code to a `clean.*` file.
- Run the benchmark (`bench/README.md`) and include the before/after precision in the PR.
