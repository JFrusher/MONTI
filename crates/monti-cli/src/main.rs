use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use monti_core::ast::Language;
use monti_core::rules::{self, CONFIG_FILE, DEFAULT_RULES, Severity};
use monti_core::{Edit, apply_fixes, scan_path};
use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[derive(Parser)]
#[command(name = "monti", version, about = "Deterministic anti-slop code sweeper")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Report findings and per-file slop scores. Exits 1 when any error-severity finding exists.
    Scan {
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
}

fn main() -> Result<ExitCode> {
    match Cli::parse().command {
        Cmd::Scan { path } => scan(&path),
        Cmd::Fix { path, interactive, no_format, no_verify } => {
            fix(&path, interactive, no_format, no_verify).map(|()| ExitCode::SUCCESS)
        }
        Cmd::Init { path } => init(&path).map(|()| ExitCode::SUCCESS),
        Cmd::Mcp => {
            let root = std::env::current_dir()?;
            monti_mcp::serve(&root, std::io::stdin().lock(), std::io::stdout().lock()).map(|()| ExitCode::SUCCESS)
        }
    }
}

fn scan(path: &Path) -> Result<ExitCode> {
    let reports = scan_path(path, &rules::load(path)?)?;
    let color = std::io::stdout().is_terminal();
    let paint = |code: &str, s: &str| if color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() };
    let (mut errors, mut warnings) = (0, 0);
    for r in reports.iter().filter(|r| !r.findings.is_empty()) {
        println!("{}  slop score {:.1}", paint("1", &r.path.display().to_string()), r.slop_score());
        for f in &r.findings {
            let severity = match f.severity {
                Severity::Error => {
                    errors += 1;
                    paint("31", "error  ")
                }
                Severity::Warning => {
                    warnings += 1;
                    paint("33", "warning")
                }
            };
            println!("  {:>5}:{:<4} {severity}  {:<22} {}", f.line, f.column, f.rule_id, f.message);
        }
        println!();
    }
    println!("{} files scanned, {errors} errors, {warnings} warnings", reports.len());
    Ok(if errors > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

struct Change {
    path: PathBuf,
    language: Language,
    original: String,
    fixed: String,
}

fn fix(path: &Path, interactive: bool, no_format: bool, no_verify: bool) -> Result<()> {
    let reports = scan_path(path, &rules::load(path)?)?;
    let mut answers = std::io::stdin().lock().lines();
    let mut changes = Vec::new();
    for r in reports {
        let original = fs::read_to_string(&r.path)?;
        let mut accepted: Vec<&Edit> = Vec::new();
        for f in &r.findings {
            let Some(edit) = &f.fix else { continue };
            if interactive {
                println!("{}:{}  [{}] {}", r.path.display(), f.line, f.rule_id, f.message);
                original[edit.start..edit.end].lines().for_each(|l| println!("  - {l}"));
                edit.replacement.lines().for_each(|l| println!("  + {l}"));
                print!("apply? [y/N] ");
                std::io::stdout().flush()?;
                if answers.next().context("stdin closed")??.trim() != "y" {
                    continue;
                }
            }
            accepted.push(edit);
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
            let bin = formatter(c.language)[0];
            ensure!(on_path(bin), "formatter `{bin}` not found; install it or pass --no-format");
        }
    }

    for c in &changes {
        fs::write(&c.path, &c.fixed)?;
    }
    let outcome = (|| -> Result<()> {
        if !no_format {
            for c in &changes {
                let [bin, args @ ..] = formatter(c.language) else { unreachable!() };
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

fn formatter(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::TypeScript | Language::Tsx | Language::JavaScript => &["prettier", "--write"],
        Language::Python => &["ruff", "format"],
        Language::Rust => &["rustfmt", "--edition", "2024"],
        Language::Go => &["gofmt", "-w"],
    }
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

fn init(dir: &Path) -> Result<()> {
    let config = dir.join(CONFIG_FILE);
    ensure!(!config.exists(), "{} already exists", config.display());
    fs::write(&config, DEFAULT_RULES)?;
    println!("wrote {}", config.display());
    Ok(())
}
