# MONTI

Deterministic anti-slop code sweeper. Named for MSG Jared C. Monti.

Languages: TypeScript, TSX, JavaScript, Python, Rust, Go, C, C++, Java, C#, Kotlin, Swift, Scala,
Dart, PHP, Ruby, Lua, GDScript, Bash, MATLAB. Adding one is a table entry plus three fixture files:
see [CONTRIBUTING.md](CONTRIBUTING.md).

It finds LLM "slop grenades" with Tree-sitter queries and AST metrics, no LLM involved:
swallowed errors (empty handlers are errors, comment-only handlers are warnings), comment floods, comments that restate code, pass-through wrappers,
redundant null checks, structurally duplicated functions and single-use abstractions.

## CLI

```sh
cargo install --path crates/monti-cli

monti scan [path]                 # findings + slop score per file; exit 1 on any error-severity finding
monti fix [path] [--interactive]  # apply fixes, format, verify with the test suite
monti init [path]                 # write the default rules to monti.toml
monti mcp                         # MCP server on stdio, rooted at the current directory
```

`monti fix` applies the deterministic fixes (delete restating comments, unwrap redundant null
checks). It then runs `prettier`, `ruff format`, `rustfmt` or `gofmt` on changed files
(`--no-format` skips), and runs the project's tests before and after (`cargo test`, `npm test`,
`pytest`, `go test ./...`, picked from the nearest manifest; `--no-verify` skips). If formatting or
tests fail after fixing, every file is restored.

Slop score = (3 × errors + warnings) per 100 code lines.

Paths in `.gitignore` and `.montiignore` (same syntax) are skipped. Files with syntax errors stop the
scan with the path; exclude them in `.montiignore`.

## Rules

`monti.toml` next to the scanned path replaces the built-in rules (`rules/default.toml`).
The table key is the rule id. Each rule sets exactly one check. `languages = ["*"]` means every
language the check supports.

| Field | Meaning |
|---|---|
| `queries` | One Tree-sitter query per language (the keys are the rule's languages). Finding at the earliest capture. Compiled at load; a bad query is an error. |
| `max_comment_code_ratio` | Comment-only lines / code lines per function. Trailing comments and tool directives (`# type: ignore`) do not count. |
| `check = "restating-comment"` | Own-line comment whose words cover every identifier of the next statement. Fixable. |
| `check = "pass-through-wrapper"` | Free function whose body is one call forwarding the parameters unchanged. Methods (they may satisfy an interface) and callbacks (they pin arity) are skipped. |
| `check = "redundant-null-check"` | Null test right after a non-null assignment (languages with a `null_check` spec). Fixable. |
| `check = "duplicate-function"` + `min_similarity` | Repo-wide Jaccard similarity of function-body AST shingles. Identifier names are ignored, literal values are not. |
| `check = "ghost-abstraction"` | Interface / abstract class / trait / protocol with one implementor and at most one other use (languages with a `ghost` spec). |

```toml
[rules.swallowed-error]
severity = "error"            # "error" | "warning"
message = "Error handler is empty."

[rules.swallowed-error.queries]
typescript = '''(catch_clause body: (statement_block) @block (#match? @block "^\\{\\s*\\}$"))'''
java = '''(catch_clause body: (block) @block (#match? @block "^\\{\\s*\\}$"))'''
```

`monti tree <file>` prints a file's syntax tree, to write queries against.

## MCP

Add to Claude Code: `claude mcp add monti -- monti mcp`.

- `monti_audit_file { path, content? }`: findings for a file or unsaved buffer.
- `monti_find_duplicate_utils { code, language }`: existing functions with the same name or a similar structure.

## GitHub Action

```yaml
- uses: actions/checkout@v4
- uses: jfrusher/monti@main
  with:
    path: .
```

Fails the job when any error-severity finding exists.

## Layout

- `crates/monti-core`: parsing, metrics, rule engine, detectors, repo index
- `crates/monti-cli`: the `monti` binary
- `crates/monti-mcp`: MCP stdio server

## License

MIT
