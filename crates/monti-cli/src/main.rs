use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use monti_core::ast::Language;
use monti_core::rules::{self, CONFIG_FILE, DEFAULT_RULES};
use monti_core::{Fix, FixLevel, apply_fixes, scan_path};
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod gate;
mod lsp;
mod output;

#[derive(Parser)]
#[command(name = "monti", version, about = "Deterministic anti-slop code sweeper")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Report findings and per-file slop scores. Exits 1 when any error-severity finding remains
    /// after `--since`, the baseline file and `monti-ignore` comments.
    Scan {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long, value_enum, default_value = "text")]
        format: output::Format,
        /// Only report findings on lines changed since this git ref (e.g. `origin/main`, `HEAD`).
        #[arg(long)]
        since: Option<String>,
    },
    /// Accept every current finding: write them to .monti-baseline.json, which `scan` then hides.
    Baseline {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Apply deterministic fixes, run the formatter, and keep them only if the test suite still passes.
    Fix {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Confirm each fix on stdin.
        #[arg(long)]
        interactive: bool,
        /// safe: delete restating comments. standard: + unwrap redundant null checks.
        /// aggressive: + inline private pass-through wrappers and merge exact same-file copies.
        #[arg(long, value_enum, default_value = "standard")]
        level: Level,
        /// Skip prettier / ruff / rustfmt / gofmt on changed files.
        #[arg(long)]
        no_format: bool,
        /// Skip running the test suite before and after fixing.
        #[arg(long)]
        no_verify: bool,
    },
    /// Write the default rules to monti.toml.
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Serve MCP over stdio, rooted at the current directory.
    Mcp,
    /// Serve the Language Server Protocol over stdio: findings as diagnostics, fixes as quick fixes.
    Lsp,
    /// Print a file's syntax tree (node kinds and field names), for writing rules and language specs.
    Tree { file: PathBuf },
}

fn main() -> Result<ExitCode> {
    match Cli::parse().command {
        Cmd::Scan { path, format, since } => scan(&path, format, since.as_deref()),
        Cmd::Baseline { path } => {
            let files = scan_path(&path, &rules::load(&path)?)?.files;
            let n = gate::write_baseline(gate::root_dir(&path), &files)?;
            println!("accepted {n} findings into {}", gate::root_dir(&path).join(gate::BASELINE_FILE).display());
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Fix { path, interactive, level, no_format, no_verify } => {
            fix(&path, interactive, level.into(), no_format, no_verify).map(|()| ExitCode::SUCCESS)
        }
        Cmd::Init { path } => init(&path).map(|()| ExitCode::SUCCESS),
        Cmd::Lsp => lsp::serve(std::io::stdin().lock(), std::io::stdout().lock()).map(|()| ExitCode::SUCCESS),
        Cmd::Tree { file } => tree(&file).map(|()| ExitCode::SUCCESS),
        Cmd::Mcp => {
            let root = std::env::current_dir()?;
            monti_mcp::serve(&root, std::io::stdin().lock(), std::io::stdout().lock()).map(|()| ExitCode::SUCCESS)
        }
    }
}

fn scan(path: &Path, format: output::Format, since: Option<&str>) -> Result<ExitCode> {
    let rules = rules::load(path)?;
    let mut scan = scan_path(path, &rules)?;
    let root = gate::root_dir(path);
    if let Some(base) = since {
        gate::keep_changed(root, base, &mut scan.files)?;
    }
    gate::apply_baseline(root, &mut scan.files)?;
    print!("{}", output::render(format, &scan, &rules));
    Ok(if output::has_errors(&scan) { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

struct Change {
    path: PathBuf,
    language: Language,
    original: String,
    fixed: String,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Level {
    Safe,
    Standard,
    Aggressive,
}

impl From<Level> for FixLevel {
    fn from(l: Level) -> FixLevel {
        match l {
            Level::Safe => FixLevel::Safe,
            Level::Standard => FixLevel::Standard,
            Level::Aggressive => FixLevel::Aggressive,
        }
    }
}

fn fix(path: &Path, interactive: bool, level: FixLevel, no_format: bool, no_verify: bool) -> Result<()> {
    let reports = scan_path(path, &rules::load(path)?)?.files;
    let mut answers = std::io::stdin().lock().lines();
    let mut changes = Vec::new();
    for r in reports {
        let original = fs::read_to_string(&r.path)?;
        let mut accepted: Vec<&Fix> = Vec::new();
        for f in &r.findings {
            let Some(fix) = f.fix.as_ref().filter(|fix| fix.level <= level) else { continue };
            if interactive {
                println!("{}:{}  [{}] {}", r.path.display(), f.line, f.rule_id, f.message);
                for e in &fix.edits {
                    let line = original[..e.start].lines().count().max(1);
                    println!("  @ line {line}");
                    original[e.start..e.end].lines().for_each(|l| println!("  - {l}"));
                    e.replacement.lines().for_each(|l| println!("  + {l}"));
                }
                print!("apply? [y/N] ");
                std::io::stdout().flush()?;
                if answers.next().context("stdin closed")??.trim() != "y" {
                    continue;
                }
            }
            accepted.push(fix);
        }
        if !accepted.is_empty() {
            let fixed = apply_fixes(&original, &accepted);
            changes.push(Change { path: r.path, language: r.language, original, fixed });
        }
    }
    if changes.is_empty() {
        println!("nothing to fix");
        return Ok(());
    }

    let suites = if no_verify { Vec::new() } else { test_suites(path)? };
    for (dir, cmd) in &suites {
        ensure!(run(cmd, dir)?, "tests fail before fixing (`{}`); fix them first or pass --no-verify", cmd.join(" "));
    }
    if !no_format {
        for c in &changes {
            if let [bin, ..] = c.language.spec().formatter {
                ensure!(on_path(bin), "formatter `{bin}` not found; install it or pass --no-format");
            }
        }
    }

    for c in &changes {
        fs::write(&c.path, &c.fixed)?;
    }
    let outcome = (|| -> Result<()> {
        if !no_format {
            for c in &changes {
                // Languages without a formatter in their spec are written as fixed.
                let [bin, args @ ..] = c.language.spec().formatter else { continue };
                ensure!(Command::new(bin).args(args).arg(&c.path).status()?.success(), "`{bin}` failed on {}", c.path.display());
            }
        }
        for (dir, cmd) in &suites {
            ensure!(run(cmd, dir)?, "tests fail after fixing (`{}`)", cmd.join(" "));
        }
        Ok(())
    })();
    if let Err(e) = outcome {
        for c in &changes {
            fs::write(&c.path, &c.original)?;
        }
        return Err(e.context("all fixes reverted"));
    }
    for c in &changes {
        println!("fixed {}", c.path.display());
    }
    Ok(())
}

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

const SUITES: &[(&str, &[&str])] = &[
    ("Cargo.toml", &["cargo", "test"]),
    ("package.json", &["npm", "test"]),
    ("pyproject.toml", &["pytest"]),
    ("pytest.ini", &["pytest"]),
    ("setup.py", &["pytest"]),
    ("go.mod", &["go", "test", "./..."]),
];

/// Test commands of the nearest ancestor directory of `path` that has a project manifest.
fn test_suites(path: &Path) -> Result<Vec<(PathBuf, &'static [&'static str])>> {
    let abs = fs::canonicalize(path)?;
    let start = if abs.is_file() { abs.parent().context("file has no parent")? } else { &abs };
    let root = start
        .ancestors()
        .find(|d| SUITES.iter().any(|(marker, _)| d.join(marker).exists()))
        .with_context(|| format!("no test suite found at or above {}; pass --no-verify to skip", start.display()))?;
    let mut cmds: Vec<&'static [&'static str]> =
        SUITES.iter().filter(|(marker, _)| root.join(marker).exists()).map(|(_, cmd)| *cmd).collect();
    cmds.dedup();
    Ok(cmds.into_iter().map(|c| (root.to_path_buf(), c)).collect())
}

fn run(cmd: &[&str], dir: &Path) -> Result<bool> {
    let status = Command::new(cmd[0]).args(&cmd[1..]).current_dir(dir).status();
    Ok(status.with_context(|| format!("running `{}`", cmd.join(" ")))?.success())
}

fn tree(file: &Path) -> Result<()> {
    let lang = Language::from_path(file).with_context(|| format!("unsupported file type: {}", file.display()))?;
    let src = fs::read_to_string(file)?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&lang.grammar())?;
    let tree = parser.parse(&src, None).context("tree-sitter returned no tree")?;
    let mut cursor = tree.walk();
    let mut depth = 0;
    loop {
        let node = cursor.node();
        let field = cursor.field_name().map(|f| format!("{f}: ")).unwrap_or_default();
        let kind = if node.is_named() { node.kind().to_string() } else { format!("{:?}", node.kind()) };
        let leaf = if node.child_count() == 0 { format!("  {:?}", &src[node.byte_range()]) } else { String::new() };
        println!("{:indent$}{field}{kind} [{}:{}]{leaf}", "", node.start_position().row + 1, node.start_position().column + 1, indent = depth * 2);
        if cursor.goto_first_child() {
            depth += 1;
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return Ok(());
            }
            depth -= 1;
        }
    }
}

fn init(dir: &Path) -> Result<()> {
    let config = dir.join(CONFIG_FILE);
    ensure!(!config.exists(), "{} already exists", config.display());
    fs::write(&config, DEFAULT_RULES)?;
    println!("wrote {}", config.display());
    Ok(())
}
