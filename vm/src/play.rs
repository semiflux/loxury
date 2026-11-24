use crate::vm::Vm;
use std::fmt::Write as _;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn run_lox(source: &str) -> String {
    let mut vm = Vm::with_outputs(Vec::new(), Vec::new());
    let mut disasm = String::new();
    let ok = vm.run(source, Some(&mut disasm)).is_ok();
    let output = String::from_utf8(std::mem::take(&mut vm.out)).unwrap_or_default();
    let error = String::from_utf8(std::mem::take(&mut vm.err)).unwrap_or_default();

    // standalone instead of serde
    // + 64 while not precise, is enough
    let mut json = String::with_capacity(disasm.len() + output.len() + error.len() + 64);
    json.push_str("{\"ok\":");
    json.push_str(if ok { "true" } else { "false" });
    json.push_str(",\"output\":[");
    for (i, line) in output.lines().enumerate() {
        if i > 0 {
            json.push(',');
        }
        push_json_str(&mut json, line);
    }
    json.push_str("],\"error\":");
    if error.is_empty() {
        json.push_str("null");
    } else {
        push_json_str(&mut json, error.trim_end());
    }
    json.push_str(",\"disasm\":");
    if ok {
        push_json_str(&mut json, &disasm);
    } else {
        json.push_str("null");
    }
    json.push('}');
    json
}

fn push_json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\0'..='\x1f' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
