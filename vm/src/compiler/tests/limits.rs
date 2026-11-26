use super::*;
use CompileError::*;

#[test]
fn function_and_method_parameter_counts_fit_in_a_byte() {
    let params = (0..255)
        .map(|i| format!("p{i}"))
        .collect::<Vec<_>>()
        .join(",");
    for (decl, end) in [("fun take", ""), ("class C { take", "}")] {
        let (script, heap) = compiled(&format!("{decl}({params}) {{ return p254; }}{end}"));
        let f = nested(&script, &heap, "take");
        assert_eq!(f.arity, 255);
        assert_eq!(Code::new(f).operand(Op::GetLocal), 255);
        let source = format!("{decl}({params},\nextra) {{}}{end}");
        assert_eq!(
            diagnostics(&source),
            [at(2, 1, TooManyParameters)],
            "{decl}"
        );
    }
}

#[test]
fn function_and_method_calls_accept_255_arguments_but_not_256() {
    let args = vec!["nil"; 255].join(",");
    for (target, method) in [("f", false), ("x.m", true)] {
        let (script, _) = compiled(&format!("{target}({args});"));
        let mut c = Code::new(&script);
        c.constant(Op::GetGlobal);
        for _ in 0..255 {
            c.op(Op::Nil);
        }
        if method {
            c.constant(Op::Invoke);
            assert_eq!(c.byte(), 255);
        } else {
            assert_eq!(c.operand(Op::Call), 255);
        }
        let source = format!("{target}({args},\nnil);");
        assert_eq!(
            diagnostics(&source),
            [at(2, 4, TooManyArguments)],
            "{target}"
        );
    }
}

#[test]
fn a_function_can_address_255_locals_in_addition_to_slot_zero() {
    let locals = (0..255).map(|i| format!("var x{i};")).collect::<String>();
    let (script, heap) = compiled(&format!("fun f() {{ {locals} return x254; }}"));
    let mut c = Code::new(nested(&script, &heap, "f"));
    for _ in 0..255 {
        c.op(Op::Nil);
    }
    assert_eq!(c.operand(Op::GetLocal), 255);
    assert_eq!(
        diagnostics(&format!("fun f() {{ {locals}\nvar extra; }}")),
        [at(2, 5, TooManyLocals)]
    );
}

#[test]
fn captured_slots_from_two_frames_share_one_byte_of_index_space() {
    let locals =
        |range: std::ops::Range<usize>| range.map(|i| format!("var v{i};")).collect::<String>();
    let reads = (0..256).map(|i| format!("v{i};")).collect::<String>();
    let prefix = format!(
        "fun a() {{ {} fun b() {{ {} fun c() {{ {reads}",
        locals(0..128),
        locals(128..257)
    );
    let (script, heap) = compiled(&format!("{prefix}\nv255; }} }} }}"));
    let a = nested(&script, &heap, "a");
    let b = nested(a, &heap, "b");
    let c = nested(b, &heap, "c");
    assert_eq!(b.upvalue_count, 128); // forwards a's locals to c
    assert_eq!(c.upvalue_count, 256); // a repeated read does not need a 257th slot
    let mut code = Code::new(c);
    for i in 0..=255 {
        assert_eq!(code.operand(Op::GetUpvalue), i);
        code.op(Op::Pop);
    }
    assert_eq!(code.operand(Op::GetUpvalue), 255);
    assert_eq!(
        diagnostics(&format!("{prefix}\nv256; }} }} }}")),
        [at(2, 1, TooManyUpvalues)]
    );
}

#[test]
fn repeating_a_literal_still_consumes_a_constant_slot() {
    let source = format!("fun f() {{ {}\n0; }}", "0;".repeat(256));
    assert_eq!(diagnostics(&source), [at(2, 1, TooManyConstants)]);
}

#[test]
fn a_loop_cannot_wrap_a_sixteen_bit_offset() {
    // Each `nil;` is two bytes. The backward edge also spans eight bytes
    // for the condition, forward branch, pop, and loop instruction itself.
    let body = "nil;\n".repeat(32763);
    compiled(&format!("while (false) {{\n{body}}}"));
    assert_eq!(
        diagnostics(&format!("while (false) {{\n{body}nil; }}")),
        [at(1, 1, JumpTooWide)]
    );
}
