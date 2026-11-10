//! MCP server over stdio (newline-delimited JSON-RPC 2.0) exposing MONTI to coding agents.

use anyhow::{Context, Result, anyhow};
use monti_core::ast::{self, Language};
use monti_core::index::Index;
use monti_core::rules::{self, Check};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

const PROTOCOL_VERSION: &str = "2025-06-18";

/// Serves requests from `input` until EOF. `root` is the workspace: config and index come from it.
pub fn serve(root: &Path, input: impl BufRead, mut output: impl Write) -> Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(root, &msg),
            Err(e) => Some(error(Value::Null, -32700, &e.to_string())),
        };
        if let Some(r) = response {
            writeln!(output, "{r}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// `None` for notifications, which get no reply.
fn handle(root: &Path, msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let result = match msg["method"].as_str().unwrap_or_default() {
        "initialize" => json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "monti", "version": env!("CARGO_PKG_VERSION") }
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => call_tool(root, &msg["params"]),
        other => return Some(error(id, -32601, &format!("method not found: {other}"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tools() -> Value {
    json!([
        {
            "name": "monti_audit_file",
            "description": "Run MONTI's deterministic anti-slop rules on one file or an unsaved buffer. Returns findings with line, column, rule, message and, when available, an exact fix edit (byte offsets).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path, relative to the workspace root or absolute. Its extension picks the language (.ts .py .rs .go)." },
                    "content": { "type": "string", "description": "Buffer contents to audit instead of the file on disk." }
                },
                "required": ["path"]
            }
        },
        {
            "name": "monti_find_duplicate_utils",
            "description": "Call before writing a new function: returns existing workspace functions with the same name or a structurally similar body, so you can reuse them instead.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": "Source of the proposed function(s)." },
                    "language": { "type": "string", "enum": ["typescript", "python", "rust", "go"] }
                },
                "required": ["code", "language"]
            }
        }
    ])
}

/// Tool failures are reported in-band (`isError`) so the agent can read and correct them.
fn call_tool(root: &Path, params: &Value) -> Value {
    let args = params["arguments"].clone();
    let outcome = match params["name"].as_str() {
        Some("monti_audit_file") => audit_file(root, args),
        Some("monti_find_duplicate_utils") => find_duplicate_utils(root, args),
        other => Err(anyhow!("unknown tool: {other:?}")),
    };
    let (text, is_error) = match outcome {
        Ok(v) => (v.to_string(), false),
        Err(e) => (format!("{e:#}"), true),
    };
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditArgs {
    path: PathBuf,
    content: Option<String>,
}

fn audit_file(root: &Path, args: Value) -> Result<Value> {
    let args: AuditArgs = serde_json::from_value(args)?;
    let path = root.join(&args.path);
    let lang = Language::from_path(&path).with_context(|| format!("unsupported file type: {}", path.display()))?;
    let src = match args.content {
        Some(c) => c,
        None => fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
    };
    let findings = monti_core::scan(lang, &src, &rules::load(root)?)?;
    Ok(json!({ "path": args.path, "findings": findings }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DuplicateArgs {
    code: String,
    language: Language,
}

fn find_duplicate_utils(root: &Path, args: Value) -> Result<Value> {
    let args: DuplicateArgs = serde_json::from_value(args)?;
    let min = rules::load(root)?
        .iter()
        .find_map(|r| match r.check {
            Check::DuplicateFunction { min_similarity } => Some(min_similarity),
            _ => None,
        })
        .context("no rule with check = \"duplicate-function\" is configured")?;
    // shortcut: re-indexes the workspace on every call; cache with file-mtime invalidation if this gets slow.
    let index = Index::build(root)?;
    let tree = ast::parse(args.language, &args.code)?;
    let matches: Vec<Value> = index
        .similar(args.language, tree.root_node(), &args.code, min)
        .iter()
        .map(|m| {
            json!({
                "proposed": m.proposed,
                "existing": m.existing.name,
                "path": m.existing.path,
                "line": m.existing.line,
                "similarity": m.similarity,
            })
        })
        .collect();
    Ok(json!({ "matches": matches }))
}
