use super::*;
use AtCoordsOrEof::Eof;
use CompileError::*;

#[test]
fn empty_source_and_comments_emit_only_the_script_return() {
    for source in [
        "",
        "// at EOF",
        "// with a newline\n",
        "// £ → ☃\n// 日本語",
    ] {
        let (f, _) = compiled(source);
        let mut c = Code::new(&f);
        c.op(Op::Nil);
        c.op(Op::Return);
        assert_eq!(c.pc, f.chunk.len());
        assert!(f.chunk.constants.is_empty());
    }
    let (f, _) = compiled("// ☃\nprint true; // EOF");
    let mut c = Code::new(&f);
    assert_eq!(f.chunk.coords(c.pc), Coords::new(2, 7));
    c.op(Op::True);
    c.op(Op::Print);
    c.op(Op::Nil);
    c.op(Op::Return);
    assert_eq!(c.pc, f.chunk.len());
}

#[test]
fn invalid_assignment_targets_point_at_the_equals_sign() {
    for (target, col) in [("(x)", 5), ("x + y", 7), ("!x", 4)] {
        assert_eq!(
            diagnostics(&format!("{target} = nil;")),
            [at(1, col, InvalidAssignmentTarget)]
        );
    }
    assert_eq!(
        diagnostics("class C { m() {\nthis = nil; } }"),
        [at(2, 6, InvalidAssignmentTarget)]
    );
}

#[test]
fn declarations_need_a_block_as_a_control_flow_body() {
    for prefix in [
        "if (true)",
        "if (true) nil; else",
        "while (false)",
        "for (;;)",
    ] {
        for decl in ["var local;", "fun local() {}", "class Local {}"] {
            let source = format!("{prefix}\n  {decl}");
            assert_eq!(
                diagnostics(&source),
                [at(2, 3, ExpectedExpression), at(2, 3, UnclosedStatement)],
                "{source}"
            );
            compiled(&format!("{prefix} {{ {decl} }}"));
        }
    }
}

#[test]
fn for_clauses_cannot_contain_block_statements() {
    for (source, errors) in [
        (
            "for (\n{}; true; nil) {}",
            vec![
                ExpectedExpression,
                UnclosedStatement,
                ExpectedExpression,
                ExpectedExpression,
            ],
        ),
        (
            "for (nil;\n{}; nil) {}",
            vec![ExpectedExpression, UnclosedStatement, ExpectedExpression],
        ),
        (
            "for (nil; true;\n{}) {}",
            vec![ExpectedExpression, ExpectedControlRightParen],
        ),
    ] {
        let expected: Vec<_> = errors.into_iter().map(|error| at(2, 1, error)).collect();
        assert_eq!(diagnostics(source), expected, "{source}");
    }
}

#[test]
fn parameters_and_locals_share_a_scope_and_require_identifiers() {
    for (source, error) in [
        ("fun f(x,\nx) {}", VariableRedeclaration("x".into())),
        ("fun f(x) { var\nx; }", VariableRedeclaration("x".into())),
        ("{ var x; var\nx; }", VariableRedeclaration("x".into())),
        (
            "{ var x =\nx; }",
            SelfReferentialVariableInitializer("x".into()),
        ),
    ] {
        assert_eq!(diagnostics(source), [at(2, 1, error)], "{source}");
    }
    for keyword in ["false", "nil", "this"] {
        let source = format!("var {keyword} = true;");
        assert_eq!(
            diagnostics(&source),
            [at(1, 5, ExpectedVariableName)],
            "{source}"
        );
    }
}

#[test]
fn function_declarations_require_a_block_and_commas_between_parameters() {
    assert_eq!(diagnostics("fun f()\n123;"), [at(2, 1, UnopenedBlock)]);
    assert_eq!(
        diagnostics("fun f(a, b\nc, d) {}"),
        [at(2, 1, UnclosedArgumentsList)]
    );
}

#[test]
fn this_and_super_require_the_right_enclosing_class() {
    for (source, error) in [
        ("\nthis;", ThisOutsideClass),
        ("fun f() {\nthis; }", ThisOutsideClass),
        ("fun f() {\nsuper.m(); }", SuperOutsideClass),
        ("class C { m() {\nsuper.m; } }", SuperWithoutClass),
        ("class C { m() {\nsuper.m(); } }", SuperWithoutClass),
        ("{ class C <\nC {} }", SelfInheritance),
        ("class C <\nC {}", SelfInheritance),
        ("class C <\n(B) {}", ExpectedClassName),
    ] {
        assert_eq!(diagnostics(source), [at(2, 1, error)], "{source}");
    }
    assert_eq!(
        diagnostics("super.m();\nsuper.m;"),
        [at(1, 1, SuperOutsideClass), at(2, 1, SuperOutsideClass)]
    );
}

#[test]
fn super_requires_a_dot_and_a_method_name() {
    for (expr, col, error) in [
        ("(super).m()", 7, ExpectedDotAfterSuper),
        ("super", 6, ExpectedDotAfterSuper),
        ("super.", 7, ExpectedMethodName),
    ] {
        let source = format!("class C < B {{ m() {{\n{expr}; }} }}");
        assert_eq!(diagnostics(&source), [at(2, col, error)], "{expr}");
    }
}

#[test]
fn invalid_literals_and_missing_expressions_locate_the_token_or_eof() {
    for (source, expected) in [
        ("7.", vec![Eof(ExpectedProperty)]),
        ("7.;", vec![at(1, 3, ExpectedProperty)]),
        (
            ".25;",
            vec![at(1, 1, ExpectedExpression), at(1, 1, UnclosedStatement)],
        ),
        ("\"no\nend", vec![Eof(UnclosedString)]),
        ("print;", vec![at(1, 6, ExpectedExpression)]),
        (
            "f(a | b);",
            vec![at(1, 5, StrayChar('|')), at(1, 7, UnclosedArgumentsList)],
        ),
    ] {
        assert_eq!(diagnostics(source), expected, "{source}");
    }
}

#[test]
fn comparison_binds_before_equality_and_negates_only_the_comparison() {
    for (operator, op, invert) in [
        ("<", Op::Less, false),
        (">", Op::Greater, false),
        ("<=", Op::Greater, true),
        (">=", Op::Less, true),
    ] {
        let (f, _) = compiled(&format!("false == 4 {operator} 5;"));
        let mut c = Code::new(&f);
        c.op(Op::False);
        assert_eq!(c.constant(Op::Constant).try_as_number(), Ok(4.0));
        assert_eq!(c.constant(Op::Constant).try_as_number(), Ok(5.0));
        c.op(op);
        if invert {
            c.op(Op::Not);
        }
        c.op(Op::Equal);
        c.op(Op::Pop);
    }
}

#[test]
fn super_lookup_and_invoke_use_the_lexical_base_but_the_current_receiver() {
    let (f, heap) = compiled("class Child < Parent { m(v) { super.pick; return super.pick(v); } }");
    let method = nested(&f, &heap, "m");
    assert_eq!(method.upvalue_count, 1); // super, not this
    let mut c = Code::new(method);
    assert_eq!(c.operand(Op::GetLocal), 0);
    assert_eq!(c.operand(Op::GetUpvalue), 0);
    let pick = c.constant(Op::GetSuper).try_as_string().unwrap();
    assert_eq!(&heap[pick], "pick");
    c.op(Op::Pop);
    assert_eq!(c.operand(Op::GetLocal), 0);
    assert_eq!(c.operand(Op::GetLocal), 1);
    assert_eq!(c.operand(Op::GetUpvalue), 0);
    assert_eq!(c.constant(Op::SuperInvoke).try_as_string(), Ok(pick));
    assert_eq!(c.byte(), 1);
    c.op(Op::Return);
}

#[test]
fn for_tests_the_condition_then_runs_the_body_before_the_first_increment() {
    let (f, _) = compiled("for (var i = true; i; i = false) nil;");
    let mut c = Code::new(&f);
    c.op(Op::True);
    let condition = c.pc;
    assert_eq!(c.operand(Op::GetLocal), 1);
    let exit = c.forward(Op::JumpIfFalse);
    c.op(Op::Pop);
    let first_body = c.forward(Op::Jump);
    let increment = c.pc;
    c.op(Op::False);
    assert_eq!(c.operand(Op::SetLocal), 1);
    c.op(Op::Pop);
    c.loops_to(condition);
    assert_eq!(first_body, c.pc);
    c.op(Op::Nil);
    c.op(Op::Pop);
    c.loops_to(increment);
    assert_eq!(exit, c.pc);
    c.op(Op::Pop); // false condition
    c.op(Op::Pop); // initializer local
    c.op(Op::Nil);
    c.op(Op::Return);
}
