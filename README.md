# MONTI

Deterministic anti-slop code sweeper. Named for MSG Jared C. Monti.

Languages: TypeScript, TSX, JavaScript, Python, Rust, Go, C, C++, Java, C#, Kotlin, Swift, Scala,
Dart, PHP, Ruby, Lua, GDScript, Bash, MATLAB. Adding one is a table entry plus three fixture files:
see [CONTRIBUTING.md](CONTRIBUTING.md).

It finds LLM "slop grenades" with Tree-sitter queries and AST metrics, no LLM involved:
swallowed errors (empty handlers are errors, comment-only handlers are warnings), silenced type checks (`as any`, bare `# type: ignore`), broad `except Exception`, calls to undefined names, unused parameters, comment floods, comments that restate code, pass-through wrappers,
redundant null checks, structurally duplicated functions and single-use abstractions.

## Install

- Binaries for Linux, macOS and Windows: [releases](https://github.com/JFrusher/MONTI/releases).
- From source: `cargo install --git https://github.com/JFrusher/MONTI monti-cli`.

## CLI

```sh

monti scan [path]                 # findings + slop score per file; exit 1 on any error-severity finding
monti scan --since origin/main    # only lines changed since a git ref (committed or not)
monti scan --format sarif         # text | json | sarif | github
monti baseline [path]             # accept today's findings (.monti-baseline.json); scan hides them
monti fix [path] [--level L] [--interactive]  # apply fixes, format, verify with the test suite
monti init [path]                 # write the default rules to monti.toml
monti mcp                         # MCP server on stdio, rooted at the current directory
```

`monti fix --level <level>` applies fixes up to that level:

| Level | Applies |
|---|---|
| `safe` | Delete restating comments. Cannot change behavior. |
| `standard` (default) | + Unwrap null checks on values just assigned a non-null value. |
| `aggressive` | + Inline private pass-through wrappers at every call site and delete them; merge exact same-file copies of a function into one. Only for file-private functions whose name appears nowhere else in the repo and is only ever called directly. |

Each fix's edits are applied together or not at all. It then runs `prettier`, `ruff format`, `rustfmt` or `gofmt` on changed files
(`--no-format` skips), and runs the project's tests before and after (`cargo test`, `npm test`,
`pytest`, `go test ./...`, picked from the nearest manifest; `--no-verify` skips). If formatting or
tests fail after fixing, every file is restored.

Slop score = (3 × errors + warnings) per 100 code lines.

### Adopting on an existing codebase

- `monti baseline` records every current finding in `.monti-baseline.json`; commit it. Later scans
  hide those and report only new slop. Entries match on rule + the line's text, so moved code stays
  accepted; edited code shows up again.
- `monti scan --since origin/main` reports only lines changed on the branch.
- Silence one spot with a comment on the line or the line above: `monti-ignore` (all rules) or
  `monti-ignore: swallowed-error, comment-flood`. Rules never see these comments.

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
| `check = "undefined-name"` | Bare call to a name never defined, imported or builtin in the file (languages with an `undefined` spec: Python). Skipped with `import *`. |
| `check = "unused-parameter"` | Free-function parameter never mentioned in the body. Skips methods, callbacks, decorated and test functions, stubs, `_names` and splats. |

```toml
[rules.swallowed-error]
severity = "error"            # "error" | "warning"
message = "Error handler is empty."

[rules.swallowed-error.queries]
typescript = '''(catch_clause body: (statement_block) @block (#match? @block "^\\{\\s*\\}$"))'''
java = '''(catch_clause body: (block) @block (#match? @block "^\\{\\s*\\}$"))'''
```

`monti tree <file>` prints a file's syntax tree, to write queries against.

Any rule also takes `skip_tests = true` (not applied to `tests/`, `*.test.*`, `test_*`, … files)
and `unless_comment_explains = true` (dropped when a comment in the flagged code gives a reason
rather than "ignore").

Rule precision is measured on 36 real repos with hand labels: see [bench/README.md](bench/README.md).

## MCP

Add to Claude Code: `claude mcp add monti -- monti mcp`.

- `monti_audit_file { path, content? }`: findings for a file or unsaved buffer.
- `monti_find_duplicate_utils { code, language }`: existing functions with the same name or a similar structure.
- `monti_repo_summary {}`: finding counts per rule and the worst files, to learn the codebase's habits.

## Editors (LSP)

`monti lsp` speaks the Language Server Protocol over stdio: findings show as diagnostics while you
type, fixes as quick fixes. Neovim:

```lua
vim.lsp.config("monti", { cmd = { "monti", "lsp" }, filetypes = { "python", "typescript", "javascript", "rust", "go" }, root_markers = { ".git" } })
vim.lsp.enable("monti")
```

Helix (`languages.toml`): `[language-server.monti] command = "monti"` with `args = ["lsp"]`, then add
`"monti"` to a language's `language-servers`.

## pre-commit

```yaml
- repo: https://github.com/JFrusher/MONTI
  rev: v0.1.1
  hooks:
    - id: monti     # runs `monti scan --since HEAD`; needs monti installed
```

## GitHub Action

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0              # only needed for `since`
- uses: JFrusher/MONTI@v0.1.1      # a release tag: the action downloads that release's binary
  with:
    path: .
    since: origin/${{ github.base_ref }}   # optional: gate only the PR's changes
```

Findings appear as annotations on the PR's lines. The job fails when any error-severity finding
remains. For GitHub code scanning, run `monti scan --format sarif > monti.sarif` and upload it with
`github/codeql-action/upload-sarif`.

## Layout

- `crates/monti-core`: parsing, metrics, rule engine, detectors, repo index
- `crates/monti-cli`: the `monti` binary
- `crates/monti-mcp`: MCP stdio server

## License

MIT
