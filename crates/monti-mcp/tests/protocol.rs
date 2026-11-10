use serde_json::{Value, json};
use std::fs;

#[path = "../../monti-core/tests/common/mod.rs"]
mod common;

fn session(root: &std::path::Path, requests: &[Value]) -> Vec<Value> {
    let input: String = requests.iter().map(|r| format!("{r}\n")).collect();
    let mut out = Vec::new();
    monti_mcp::serve(root, input.as_bytes(), &mut out).unwrap();
    String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn tool_json(response: &Value) -> Value {
    assert_eq!(response["result"]["isError"], false, "{response}");
    serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn audit_and_duplicate_tools_over_stdio() {
    let root = common::temp_dir("mcp");
    fs::write(root.join("util.py"), "def clamp(value, low, high):\n    if value < low:\n        return low\n    if value > high:\n        return high\n    return value\n").unwrap();

    let call = |id: u64, name: &str, args: Value| {
        json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": args}})
    };
    let responses = session(
        &root,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            call(3, "monti_audit_file", json!({"path": "buffer.py", "content": "try:\n    go()\nexcept Exception:\n    pass\n"})),
            call(4, "monti_find_duplicate_utils", json!({"language": "python", "code": "def bound(x, lo, hi):\n    if x < lo:\n        return lo\n    if x > hi:\n        return hi\n    return x\n"})),
            call(5, "monti_audit_file", json!({"path": "notes.txt"})),
            json!({"jsonrpc": "2.0", "id": 6, "method": "nope"}),
        ],
    );

    assert_eq!(responses.len(), 6, "the notification must not be answered");
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "monti");
    let names: Vec<&str> = responses[1]["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["monti_audit_file", "monti_find_duplicate_utils"]);

    let audit = tool_json(&responses[2]);
    assert_eq!(audit["findings"][0]["rule_id"], "swallowed-error");
    assert_eq!(audit["findings"][0]["line"], 3);

    let dup = tool_json(&responses[3]);
    assert_eq!(dup["matches"][0]["existing"], "clamp");
    assert_eq!(dup["matches"][0]["similarity"], 1.0);

    assert_eq!(responses[4]["result"]["isError"], true);
    assert_eq!(responses[5]["error"]["code"], -32601);
}
