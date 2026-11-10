//! `monti lsp`: Language Server Protocol over stdio. Findings become diagnostics while you edit;
//! fixes become quick-fix code actions.

use anyhow::{Context, Result};
use monti_core::ast::Language;
use monti_core::rules::{self, Rule, Severity};
use monti_core::{Edit, Finding};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;

pub fn serve(mut input: impl BufRead, mut output: impl Write) -> Result<()> {
    let mut docs: HashMap<String, String> = HashMap::new();
    let mut rules: Vec<Rule> = Vec::new();
    while let Some(msg) = read_message(&mut input)? {
        let id = msg.get("id").cloned();
        let params = &msg["params"];
        match msg["method"].as_str().unwrap_or_default() {
            "initialize" => {
                let root = params["rootUri"].as_str().map_or(std::env::current_dir()?, uri_to_path);
                let response = match rules::load(&root) {
                    Ok(loaded) => {
                        rules = loaded;
                        json!({ "result": {
                            "capabilities": { "textDocumentSync": 1, "codeActionProvider": true },
                            "serverInfo": { "name": "monti", "version": env!("CARGO_PKG_VERSION") },
                        }})
                    }
                    Err(e) => json!({ "error": { "code": -32603, "message": format!("{e:#}") } }),
                };
                send(&mut output, merge(json!({ "jsonrpc": "2.0", "id": id }), response))?;
            }
            "textDocument/didOpen" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default().to_string();
                docs.insert(uri.clone(), params["textDocument"]["text"].as_str().unwrap_or_default().to_string());
                publish(&mut output, &uri, &docs[&uri], &rules)?;
            }
            "textDocument/didChange" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default().to_string();
                // Full sync (textDocumentSync = 1): the last change holds the whole text.
                if let Some(text) = params["contentChanges"].as_array().and_then(|c| c.last()).and_then(|c| c["text"].as_str()) {
                    docs.insert(uri.clone(), text.to_string());
                    publish(&mut output, &uri, &docs[&uri], &rules)?;
                }
            }
            "textDocument/didClose" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                docs.remove(uri);
                notify(&mut output, "textDocument/publishDiagnostics", json!({ "uri": uri, "diagnostics": [] }))?;
            }
            "textDocument/codeAction" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                let actions = docs.get(uri).map_or(Vec::new(), |text| code_actions(uri, text, &rules, &params["range"]));
                send(&mut output, json!({ "jsonrpc": "2.0", "id": id, "result": actions }))?;
            }
            "shutdown" => send(&mut output, json!({ "jsonrpc": "2.0", "id": id, "result": null }))?,
            "exit" => return Ok(()),
            method if id.is_some() => send(
                &mut output,
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("method not found: {method}") } }),
            )?,
            _ => {}
        }
    }
    Ok(())
}

fn merge(mut a: Value, b: Value) -> Value {
    if let (Some(a), Value::Object(b)) = (a.as_object_mut(), b) {
        a.extend(b);
    }
    a
}

/// `None` when the file is not a supported language or does not parse (mid-edit): callers then
/// leave the previous diagnostics in place instead of flickering them away.
fn findings(uri: &str, text: &str, rules: &[Rule]) -> Option<Vec<Finding>> {
    let path = uri_to_path(uri);
    let lang = Language::from_path(&path)?;
    monti_core::scan(lang, &path, text, rules).ok()
}

fn publish(out: &mut impl Write, uri: &str, text: &str, rules: &[Rule]) -> Result<()> {
    let Some(findings) = findings(uri, text, rules) else { return Ok(()) };
    let lines: Vec<&str> = text.lines().collect();
    let diagnostics: Vec<Value> = findings
        .iter()
        .map(|f| {
            let line = lines.get(f.line - 1).copied().unwrap_or("");
            let start = utf16(&line[..(f.column - 1).min(line.len())]);
            json!({
                "range": { "start": { "line": f.line - 1, "character": start },
                           "end": { "line": f.line - 1, "character": utf16(line) } },
                "severity": match f.severity { Severity::Error => 1, Severity::Warning => 2 },
                "source": "monti",
                "code": f.rule_id,
                "message": f.message,
            })
        })
        .collect();
    notify(out, "textDocument/publishDiagnostics", json!({ "uri": uri, "diagnostics": diagnostics }))
}

fn code_actions(uri: &str, text: &str, rules: &[Rule], range: &Value) -> Vec<Value> {
    let first = range["start"]["line"].as_u64().unwrap_or(0) as usize + 1;
    let last = range["end"]["line"].as_u64().unwrap_or(0) as usize + 1;
    let Some(findings) = findings(uri, text, rules) else { return Vec::new() };
    findings
        .iter()
        .filter(|f| (first..=last).contains(&f.line))
        .filter_map(|f| {
            let fix = f.fix.as_ref()?;
            let edits: Vec<Value> = fix.edits.iter().map(|e| text_edit(text, e)).collect();
            Some(json!({
                "title": format!("monti: fix {} ({:?})", f.rule_id, fix.level).to_lowercase(),
                "kind": "quickfix",
                "edit": { "changes": { uri: edits } },
            }))
        })
        .collect()
}

fn text_edit(text: &str, e: &Edit) -> Value {
    json!({ "range": { "start": position(text, e.start), "end": position(text, e.end) }, "newText": e.replacement })
}

/// LSP positions count UTF-16 code units within the line.
fn position(text: &str, byte: usize) -> Value {
    let before = &text[..byte];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    json!({ "line": before.matches('\n').count(), "character": utf16(&before[line_start..]) })
}

fn utf16(s: &str) -> usize {
    s.encode_utf16().count()
}

fn uri_to_path(uri: &str) -> PathBuf {
    let raw = uri.strip_prefix("file://").unwrap_or(uri);
    let mut bytes = Vec::with_capacity(raw.len());
    let mut it = raw.bytes();
    while let Some(b) = it.next() {
        let hex = |c: Option<u8>| c.and_then(|c| (c as char).to_digit(16));
        if b == b'%'
            && let (Some(h), Some(l)) = (hex(it.clone().next()), hex(it.clone().nth(1)))
        {
            bytes.push((h * 16 + l) as u8);
            it.nth(1);
        } else {
            bytes.push(b);
        }
    }
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_message(input: &mut impl BufRead) -> Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut header = String::new();
        if input.read_line(&mut header)? == 0 {
            return Ok(None);
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some(v) = header.strip_prefix("Content-Length:") {
            length = Some(v.trim().parse::<usize>()?);
        }
    }
    let mut body = vec![0; length.context("LSP message without Content-Length")?];
    input.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

fn send(out: &mut impl Write, msg: Value) -> Result<()> {
    let body = msg.to_string();
    write!(out, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    out.flush()?;
    Ok(())
}

fn notify(out: &mut impl Write, method: &str, params: Value) -> Result<()> {
    send(out, json!({ "jsonrpc": "2.0", "method": method, "params": params }))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    fn frame(msg: Value) -> String {
        let body = msg.to_string();
        format!("Content-Length: {}\r\n\r\n{body}", body.len())
    }

    fn replies(out: &[u8]) -> Vec<Value> {
        let mut input = out;
        std::iter::from_fn(|| super::read_message(&mut input).unwrap()).collect()
    }

    #[test]
    fn diagnostics_and_quick_fixes() {
        let src = "def f():\n    # increment count\n    count += 1\n    try:\n        print()\n    except Exception:\n        pass\n";
        let uri = "file:///tmp/monti%20lsp/a.py";
        let input = [
            frame(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"rootUri": null}})),
            frame(json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri, "languageId": "python", "version": 1, "text": src}}})),
            frame(json!({"jsonrpc": "2.0", "method": "textDocument/didChange", "params": {"textDocument": {"uri": uri, "version": 2}, "contentChanges": [{"text": "def (:\n"}]}})),
            frame(json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/codeAction", "params": {"textDocument": {"uri": uri}, "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 0}}, "context": {"diagnostics": []}}})),
            frame(json!({"jsonrpc": "2.0", "id": 3, "method": "shutdown"})),
            frame(json!({"jsonrpc": "2.0", "method": "exit"})),
        ]
        .concat();
        let mut out = Vec::new();
        super::serve(input.as_bytes(), &mut out).unwrap();
        let replies = replies(&out);

        assert_eq!(replies[0]["result"]["capabilities"]["codeActionProvider"], true);
        let diags = replies[1]["params"]["diagnostics"].as_array().unwrap();
        let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
        assert_eq!(codes, ["restating-comment", "swallowed-error"]);
        assert_eq!(diags[1]["severity"], 1);
        // The unparseable edit published nothing, so the next reply is the code action response:
        // it ran on the broken text, which has no findings.
        assert_eq!(replies[2]["id"], 2);
        assert_eq!(replies[2]["result"], json!([]));
        assert_eq!(replies[3]["id"], 3);
        assert_eq!(super::uri_to_path(uri), std::path::PathBuf::from("/tmp/monti lsp/a.py"));
    }

    #[test]
    fn code_action_deletes_restating_comment() {
        let src = "def f():\n    # increment count\n    count += 1\n";
        let rules = monti_core::rules::parse_rules(monti_core::rules::DEFAULT_RULES).unwrap();
        let range = json!({"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 0}});
        let actions = super::code_actions("file:///a.py", src, &rules, &range);
        assert_eq!(actions[0]["title"], "monti: fix restating-comment (safe)");
        assert_eq!(
            actions[0]["edit"]["changes"]["file:///a.py"][0],
            json!({"range": {"start": {"line": 1, "character": 0}, "end": {"line": 2, "character": 0}}, "newText": ""})
        );
    }
}
